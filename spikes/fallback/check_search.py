#!/usr/bin/env python3
"""Native corpus backfill, cross-store crash reconciliation and indexed queries."""
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
    (output / 'OWNER').write_text('akasha-otsuge MVP-003 fallback search receipts\n')
    records, started = [], time.monotonic()
    with tempfile.TemporaryDirectory(prefix='akasha-fallback-search-') as temporary:
        root = pathlib.Path(temporary)
        (root / 'OWNER').write_text('akasha-otsuge MVP-003 owned corpus import and database\n')
        database, source, manifest, request = [root / n for n in ('database', 'source.jsonl', 'manifest.json', 'request.json')]
        publish(output / 'admission.json', dict(binary_sha256=digest(binary),
            harness_sha256=digest(pathlib.Path(__file__)), controller=process_identity(os.getpid()),
            temporary_root=temporary, scope='1057 synthetic corpus records; excluded from full-scale accounting'))
        sha, size, body_size = hashlib.sha256(), 0, 0
        with source.open('xb') as stream:
            for i in range(1057):
                body = ('beaconfortytwo ' if i == 42 else 'ordinary astronomy ') + 'x' * 20480
                row = dict(key=f'search:{i}', body=body, partition=f'p{i % 8}',
                           embedding=[float(j == i) for j in range(1536)])
                raw = (json.dumps(row, separators=(',', ':')) + '\n').encode()
                stream.write(raw); sha.update(raw); size += len(raw); body_size += len(body)
        manifest.write_text(json.dumps(dict(corpus=dict(documents=1057, vectors=1057,
            prepared_bytes=size, prepared_sha256=sha.hexdigest(), indexed_body_utf8_bytes=body_size))))

        def invoke(*arguments, kill=None, error=None):
            name = f'{len(records):03d}'
            command = [str(binary), str(database), *map(str, arguments)]
            with (output / (name + '.stdout')).open('xb') as stdout:
                child = subprocess.Popen(command, stdout=stdout, stderr=subprocess.PIPE,
                    env=dict(os.environ, AKASHA_SPIKE_PROFILE='low', LANCE_CPU_THREADS='2'))
                identity, prefix = process_identity(child.pid), b''
                try:
                    if kill:
                        with selectors.DefaultSelector() as selector:
                            selector.register(child.stderr, selectors.EVENT_READ)
                            until = time.monotonic() + 180
                            while time.monotonic() < until:
                                assert child.poll() is None, prefix
                                if selector.select(timeout=1):
                                    prefix += os.read(child.stderr.fileno(), 4096)
                                    if f'PROBE_CHECKPOINT:{kill}\n'.encode() in prefix:
                                        break
                            else:
                                raise TimeoutError('derived checkpoint absent')
                        assert process_identity(child.pid)['start_ticks'] == identity['start_ticks']
                        child.kill()
                    _, tail = child.communicate(timeout=600)
                    (output / (name + '.stderr')).write_bytes(prefix + tail)
                    assert child.returncode == (-9 if kill else 1 if error else 0), (name, tail[-2000:])
                    if error:
                        assert error.encode() in tail, (name, tail[-2000:])
                finally:
                    if child.poll() is None:
                        child.kill()
                    child.communicate(timeout=10)
            assert process_identity(child.pid) is None
            records.append(dict(command=command, child=identity, exit=child.returncode,
                request=json.loads(request.read_text()) if arguments[0] == 'derive' else None,
                kill=kill, expected_error=error))
            elapsed = time.monotonic() - started
            publish(output / 'progress.json', dict(phase=name, completed=len(records), total=39,
                elapsed_s=elapsed, rate_per_s=len(records)/elapsed,
                eta_s=max(0, 39-len(records))*elapsed/len(records)))
            if not kill and not error:
                return json.loads((output / (name + '.stdout')).read_text())

        def call(op, generation=1, kill=None, error=None, **fields):
            request.write_text(json.dumps(dict(op=op, generation=generation, **fields)))
            return invoke('derive', request, kill=kill, error=error)

        bundle = dict(model='synthetic-basis-1536', model_fingerprint='a' * 64,
                      dimension=1536, chunker='whole-record-v1', schema=1)
        assert invoke('--load', source, manifest)['documents'] == 1057
        state = call('begin', bundle=bundle)
        assert state['cursor'] == 0 and not state['ready']
        assert call('begin', bundle=bundle) == state
        call('begin', bundle=dict(bundle, model='different'), error='generation identity conflict')
        call('query', bundle=bundle, scope=2, term='beaconfortytwo', error='not ready')
        state = call('backfill', waves=100)
        assert state['cursor'] == state['vector_rows'] == 1057 and state['pending'] is None
        ready = call('build')
        assert ready['ready'] and call('build') == ready
        assert call('query', bundle=bundle, scope=2, term='beaconfortytwo')['ids'] == [1_000_042]
        assert call('query', bundle=bundle, scope=0, term='beaconfortytwo')['ids'] == []
        vector = [1.0] + [0.0] * 1535
        assert call('query', bundle=bundle, scope=0, vector=vector)['ids'][0] == 1_000_000
        call('query', bundle=dict(bundle, model='other'), scope=0, vector=vector, error='bundle mismatch')
        call('query', bundle=bundle, scope=0, vector=[0.0] * 1536, error='invalid search request')
        call('backfill', waves=0, error='invalid wave count')
        for generation, barrier in enumerate(('before-text', 'after-text', 'after-vector', 'after-cursor'), 2):
            call('begin', generation, bundle=bundle)
            call('backfill', generation, waves=100, hold=barrier, kill=barrier)
            interrupted = call('status', generation)
            assert interrupted['cursor'] < 1057 and not interrupted['ready']
            state = call('backfill', generation, waves=100)
            assert state['cursor'] == state['vector_rows'] == 1057 and state['pending'] is None
            ready = call('build', generation)
            assert ready['ready']
            assert call('query', generation, bundle=bundle, scope=0, vector=vector)['ids'][0] == 1_000_000
        assert invoke('verify-import')['prefix'] == sha.hexdigest()
        assert len(records) == 39, len(records)
    elapsed = time.monotonic() - started
    publish(output / 'progress.json', dict(phase='completed', completed=39, total=39,
        elapsed_s=elapsed, rate_per_s=39/elapsed, eta_s=0))
    publish(output / 'result.json', dict(status='passed', checks=39, seconds=elapsed, records=records,
        temporary_root_removed=not pathlib.Path(temporary).exists(),
        artifact_sha256={str(p.relative_to(output)):digest(p) for p in output.iterdir() if p.is_file()},
        not_tested=['full corpus', 'ordinary concurrent changes', 'active generation switch', 'power loss']))
    print(json.dumps(dict(status='passed', checks=39, seconds=elapsed)))


if __name__ == '__main__':
    main()

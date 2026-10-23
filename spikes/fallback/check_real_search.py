#!/usr/bin/env python3
"""Verify real baseline queries against immutable authority in all eight scopes."""
import fcntl
import hashlib
import json
import os
import pathlib
import struct
import subprocess
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'helix'))
from run_case import digest, process_identity, publish


def main():
    binary, database, fixture, output = [pathlib.Path(p).resolve() for p in sys.argv[1:]]
    if fixture.stat().st_size > 8 * 1024**2:
        raise ValueError('query fixture exceeds 8 MiB')
    population = json.loads(fixture.read_text())
    sidecar = population['vector_file']
    if pathlib.Path(sidecar['name']).name != sidecar['name'] or population['dimension'] != 1536:
        raise ValueError('invalid vector sidecar')
    vectors = fixture.parent / sidecar['name']
    assert vectors.stat().st_size == sidecar['bytes'] and digest(vectors) == sidecar['sha256']
    owner = (database / '.akasha-case-owner.lock').open('a')
    fcntl.flock(owner, fcntl.LOCK_EX | fcntl.LOCK_NB)
    prior = json.loads((database / '.akasha-generation-owner.json').read_text())
    assert json.loads((pathlib.Path(prior['output']) / 'state.json').read_text())['phase'] == 'completed'
    output.mkdir(mode=0o700)
    (output / 'OWNER').write_text('akasha-otsuge MVP-003 real baseline query receipts\n')
    publish(output / 'admission.json', dict(binary_sha256=digest(binary), fixture_sha256=digest(fixture),
        vector_sha256=digest(vectors), harness_sha256=digest(pathlib.Path(__file__)),
        controller=process_identity(os.getpid()), database=str(database)))
    records, checks, cached = [], [], {}
    started = time.monotonic()

    def invoke(*args):
        command = [str(binary), str(database), *map(str, args)]
        child = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            env={**os.environ, 'AKASHA_SPIKE_PROFILE':'low', 'LANCE_CPU_THREADS':'2'})
        identity = process_identity(child.pid)
        try:
            stdout, stderr = child.communicate(timeout=180)
        finally:
            if child.poll() is None:
                child.kill()
                child.communicate(timeout=10)
        name = f'{len(records):03d}'
        (output / (name + '.stderr')).write_bytes(stderr)
        records.append(dict(command=command, child=identity, exit=child.returncode,
            stdout_sha256=hashlib.sha256(stdout).hexdigest()))
        publish(output / 'commands.json', records)
        assert child.returncode == 0 and process_identity(child.pid) is None, stderr[-2000:]
        value = json.loads(stdout)
        # Source records remain in authority; retain hashes and compact query replies only.
        if args[0] != 'corpus-get':
            (output / (name + '.stdout')).write_bytes(stdout)
        return value

    def call(request):
        path = output / 'request.json'
        publish(path, request)
        return invoke('--generation', path)

    state = call(dict(op='status', generation=1))
    assert state['ready'] and state['cursor'] == state['documents']
    bundle = state['bundle']
    selected = {}
    for row in population['queries']:
        if row.get('vector_offset') is not None and 1_000_000 <= row['memory'] < 1_000_000 + state['documents']:
            selected.setdefault(row['partition'], row)
    assert set(selected) == {f'p{i}' for i in range(8)}
    with vectors.open('rb') as vector_file:
        for scope, query in sorted(selected.items()):
            document = invoke('corpus-get', query['memory'])
            assert document['key'] == query['key'] and document['partition'] == scope
            assert hashlib.sha256(document['body'].encode()).hexdigest() == query['body_sha256']
            offset = query['vector_offset']
            assert type(offset) is int and offset >= 0 and offset % (1536 * 4) == 0
            vector_file.seek(offset)
            vector = vector_file.read(1536 * 4)
            assert struct.pack('<1536f', *document['embedding']) == vector
            for mode, fields in [('bm25', dict(term=query['term'])),
                                 ('ann', dict(vector=list(struct.unpack('<1536f', vector))))]:
                request = dict(op='query', generation=1, bundle=bundle, scope=int(scope[1]), **fields)
                reply = call(request)
                ids = reply['ids']
                assert reply['corpus_baseline_only'] and 0 < len(ids) <= 20 and len(ids) == len(set(ids))
                if mode == 'ann':
                    assert query['memory'] in ids, (scope, query['memory'], ids)
                for memory in ids:
                    if memory not in cached:
                        doc = invoke('corpus-get', memory)
                        cached[memory] = dict(partition=doc['partition'], key=doc['key'],
                            body_sha256=hashlib.sha256(doc['body'].encode()).hexdigest())
                    assert cached[memory]['partition'] == scope
                checks.append(dict(mode=mode, partition=scope, query=query, ids=ids,
                    self_at_20=query['memory'] in ids))
                elapsed = time.monotonic() - started
                publish(output / 'progress.json', dict(completed=len(checks), total=16, elapsed_s=elapsed,
                    rate_per_s=len(checks)/elapsed, eta_s=(16-len(checks))*elapsed/len(checks),
                    **process_identity(os.getpid())))
    publish(output / 'result.json', dict(status='passed', queries=checks, checked_candidates=cached,
        native_commands=len(records), seconds=time.monotonic()-started, bundle=bundle,
        artifact_sha256={p.name:digest(p) for p in output.iterdir() if p.is_file()},
        not_proven=['full corpus', 'ANN exact recall', 'BM25 expected rank', 'cold cache', 'live permissions']))
    print(json.dumps(dict(status='passed', real_queries=len(checks), native_commands=len(records))))


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Verify real baseline queries against immutable authority in all eight scopes."""
import argparse
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


def selected_queries(population, documents):
    rows = population['queries']
    eligible = [q for q in rows if q.get('vector_offset') is not None
                and 1_000_000 <= q['memory'] < 1_000_000 + documents]
    if any('exact_neighbors' in q for q in rows):
        if len(eligible) != len(rows) or len({q['memory'] for q in rows}) != len(rows):
            raise ValueError('reference queries must all identify distinct corpus rows')
        for query in rows:
            expected = query.get('exact_neighbors')
            if (not isinstance(expected, list) or len(expected) != 20
                    or not all(isinstance(key, str) and key for key in expected)
                    or len(set(expected)) != 20):
                raise ValueError('every reference query requires twenty distinct exact neighbors')
        selected = eligible
    else:
        scopes = {}
        for query in eligible:scopes.setdefault(query['partition'], query)
        selected = [scopes[key] for key in sorted(scopes)]
    if {q['partition'] for q in selected} != {f'p{i}' for i in range(8)}:
        raise ValueError('query population must cover every scope')
    return selected


def run_native(command, output, records, timeout=180, retain_stdout=True):
    name = f'{len(records):03d}'
    child = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        env={**os.environ, 'AKASHA_SPIKE_PROFILE':'low', 'LANCE_CPU_THREADS':'2'})
    record = dict(command=command, child=process_identity(child.pid), phase='running')
    records.append(record)
    publish(output / 'commands.json', records)
    started, timed_out = time.monotonic(), False
    try:
        try:
            stdout, stderr = child.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            child.kill()
            stdout, stderr = child.communicate(timeout=10)
    finally:
        if child.poll() is None:
            child.kill()
            child.communicate(timeout=10)
    (output / (name + '.stderr')).write_bytes(stderr)
    if retain_stdout:
        (output / (name + '.stdout')).write_bytes(stdout)
    record.update(phase='completed', exit=child.returncode, timed_out=timed_out,
        seconds=time.monotonic()-started, stdout_sha256=hashlib.sha256(stdout).hexdigest())
    publish(output / 'commands.json', records)
    return stdout, stderr


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['binary', 'database', 'fixture', 'output']:
        parser.add_argument(name, type=pathlib.Path)
    parser.add_argument('--generation', type=int, choices=[1, 2], default=1)
    args = parser.parse_args()
    binary, database, fixture, output = [getattr(args, key).resolve()
        for key in ['binary', 'database', 'fixture', 'output']]
    generation = args.generation
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
        controller=process_identity(os.getpid()), database=str(database), generation=generation))
    records, checks, cached = [], [], {}
    started = time.monotonic()

    def invoke(*args):
        command = [str(binary), str(database), *map(str, args)]
        # Source bodies stay in authority; compact replies and failures retain raw output.
        stdout, stderr = run_native(command, output, records, retain_stdout=args[0] != 'corpus-get')
        record = records[-1]
        assert not record['timed_out'] and record['exit'] == 0, record
        assert process_identity(record['child']['pid']) is None, record
        return json.loads(stdout)

    def call(request):
        path = output / 'request.json'
        publish(path, request)
        return invoke('--generation', path)

    state = call(dict(op='status', generation=generation))
    assert state['ready'] and state['cursor'] == state['documents']
    bundle = state['bundle']
    selected = selected_queries(population, state['documents'])
    if any('exact_neighbors' in q for q in selected):
        assert population['corpus_manifest_sha256'] == state['source'], 'reference corpus identity differs'
    publish(output / 'progress.json', dict(completed=0, total=2*len(selected), elapsed_s=0,
        rate_per_s=0, eta_s=None, **process_identity(os.getpid())))
    with vectors.open('rb') as vector_file:
        for query in selected:
            scope = query['partition']
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
                request = dict(op='query', generation=generation, bundle=bundle, scope=int(scope[1]), **fields)
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
                recall = (len({cached[memory]['key'] for memory in ids} & set(query['exact_neighbors'])) / 20
                          if mode == 'ann' and 'exact_neighbors' in query else None)
                checks.append(dict(mode=mode, partition=scope, query=query, ids=ids,
                    self_at_20=query['memory'] in ids, ann_recall_at_20=recall))
                elapsed = time.monotonic() - started
                publish(output / 'progress.json', dict(completed=len(checks), total=2*len(selected), elapsed_s=elapsed,
                    rate_per_s=len(checks)/elapsed, eta_s=(2*len(selected)-len(checks))*elapsed/len(checks),
                    **process_identity(os.getpid())))
    recalls = [row['ann_recall_at_20'] for row in checks if row['ann_recall_at_20'] is not None]
    publish(output / 'result.json', dict(status='passed', queries=checks, checked_candidates=cached,
        native_commands=len(records), seconds=time.monotonic()-started, bundle=bundle,
        exact_ann_queries=len(recalls), mean_ann_recall_at_20=sum(recalls)/len(recalls) if recalls else None,
        artifact_sha256={p.name:digest(p) for p in output.iterdir() if p.is_file()},
        not_proven=['BM25 expected rank', 'cold cache', 'live permissions', 'sustained workload']
                   + ([] if recalls else ['full corpus', 'ANN exact recall'])))
    print(json.dumps(dict(status='passed', real_queries=len(checks), native_commands=len(records))))


if __name__ == '__main__':
    main()

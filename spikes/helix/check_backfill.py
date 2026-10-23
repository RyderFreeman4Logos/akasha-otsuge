#!/usr/bin/env python3
"""Pinned imported corpus, atomic backfill cursor and native generation reads."""
import hashlib
import json
import os
import pathlib
import re
import selectors
import subprocess
import sys
import tempfile
import time

from run_case import digest, process_identity, publish


def main():
    binary, source = [pathlib.Path(p).resolve(strict=True) for p in sys.argv[1:3]]
    output = pathlib.Path(sys.argv[3]).resolve()
    output.mkdir(mode=0o700)
    (output / 'OWNER').write_text('akasha-otsuge MVP-003 corpus backfill receipts\n')
    started = time.monotonic()
    records = []
    planned_commands = 146
    rows = [json.loads(line) for line in source.read_text().splitlines()]
    assert len(rows) == 8 and source.stat().st_size < 4 * 1024**2
    rows += [{'key': f'fixture:{i}', 'body': f'Synthetic backfill diagnostic only {i}', 'partition': 'p0'} for i in range(17)]
    identity = {'model': 'text-embedding-3-large', 'repository': 'Qdrant/dbpedia-entities-openai3-text-embedding-3-large-1536-1M', 'revision': '4a9731217921bc476a0f03544f11f22ae4903fa5', 'precomputed_at': '2024-02', 'precision': 'float32'}
    fingerprint = hashlib.sha256(json.dumps(identity, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    bundle = {'model': identity['model'], 'model_fingerprint': fingerprint, 'dimension': 1536, 'chunker': 'source-document-v1', 'schema': 1}
    data = b''.join((json.dumps(row, separators=(',', ':')) + '\n').encode() for row in rows)
    prepared = output / 'input.jsonl'
    prepared.write_bytes(data)
    manifest = output / 'manifest.json'
    manifest.write_text(json.dumps({'dimensions': 1536, 'embedding_model': identity['model'], 'precomputed_at': '2024-02 (publisher)', 'chunk_bytes': 65536, 'corpus': {'documents': len(rows), 'vectors': 8, 'prepared_bytes': len(data), 'prepared_sha256': hashlib.sha256(data).hexdigest(), 'indexed_body_utf8_bytes': sum(len(r['body'].encode()) for r in rows), 'inputs': [{'file': 'vectors-00000.parquet', 'repository': identity['repository'], 'revision': identity['revision']}]}}))
    with tempfile.TemporaryDirectory(prefix='akasha-backfill-') as directory:
        root = pathlib.Path(directory)
        (root / 'OWNER').write_text('akasha-otsuge MVP-003 disposable corpus backfill database\n')
        database = root / 'database'
        admission = {'controller': process_identity(os.getpid()), 'binary_sha256': digest(binary), 'source_sha256': digest(source), 'harness_sha256': digest(pathlib.Path(__file__)), 'temporary_root': directory, 'planned_commands': planned_commands}
        publish(output / 'admission.json', admission)

        def invoke(request, *, kill=None, failure=False):
            name = f'{len(records):03d}-{request["op"]}'
            if kill:
                request = {**request, 'hold': kill}
            path = output / (name + '.json')
            path.write_text(json.dumps(request))
            if request['op'] == 'load':
                args = ['--load', str(prepared), str(manifest)]
            elif request['op'] == 'raw':
                args = [request['cypher']]
            else:
                args = ['--generation', str(path)]
            with (output / (name + '.log')).open('x') as log:
                child = subprocess.Popen([str(binary), str(database), *args], stdout=log, stderr=subprocess.PIPE, bufsize=0)
                child_id = process_identity(child.pid)
                try:
                    stderr = b''
                    if kill:
                        with selectors.DefaultSelector() as selector:
                            selector.register(child.stderr, selectors.EVENT_READ)
                            deadline = time.monotonic() + 90
                            while time.monotonic() < deadline:
                                assert child.poll() is None, stderr + child.stderr.read()
                                if selector.select(timeout=1):
                                    stderr += os.read(child.stderr.fileno(), 4096)
                                    if ('PROBE_CHECKPOINT:' + kill + '\n').encode() in stderr:
                                        break
                            else:
                                raise TimeoutError(kill)
                        child.kill()
                    _, tail = child.communicate(timeout=120)
                    stderr += tail
                finally:
                    if child.poll() is None:
                        child.kill()
                    child.communicate(timeout=10)
            (output / (name + '.stderr')).write_bytes(stderr)
            records.append({'request': path.name, 'child': child_id, 'exit': child.returncode, 'hold': kill, 'expected_failure': failure})
            elapsed = time.monotonic() - started
            publish(output / 'progress.json', {'phase': name, 'completed': len(records), 'total': planned_commands, 'elapsed_s': elapsed, 'rate_per_s': len(records) / elapsed, 'eta_s': (planned_commands - len(records)) * elapsed / len(records)})
            assert child.returncode == -9 if kill else (child.returncode != 0) == failure, stderr.decode(errors='replace')
            if kill or failure:
                return None
            value = json.loads((output / (name + '.log')).read_text().splitlines()[-1])
            records[-1]['result'] = value
            return value

        invoke({'op': 'load'})
        begin = {'op': 'begin_corpus', 'generation': 1, 'bundle': bundle, 'manifest': str(manifest)}
        invoke({'op': 'raw', 'cypher': 'MATCH (s:ImportState) SET s.complete=false'})
        invoke(begin, failure=True)
        invoke({'op': 'raw', 'cypher': 'MATCH (s:ImportState) SET s.complete=true'})
        first = invoke(begin)
        assert first['base']['completed'] == 0
        invoke({'op': 'raw', 'cypher': 'MATCH (d:Document {memory:1000002}) SET d.memory=1000001'})
        invoke({'op': 'current', 'principal': 'reader', 'memory': 1_000_001}, failure=True)
        invoke({'op': 'backfill', 'generation': 1, 'waves': 1}, failure=True)
        invoke({'op': 'raw', 'cypher': f'MATCH (d:Document {{key:{json.dumps(rows[2]["key"])}}}) SET d.memory=1000002'})
        assert invoke(begin)['base']['completed'] == 0
        invoke({'op': 'raw', 'cypher': "CREATE (:Document {memory:99,key:'stray',body:'excluded fixture',partition:'p0'})"})
        invoke(begin, failure=True)
        invoke({'op': 'raw', 'cypher': 'MATCH (d:Document {memory:99}) DELETE d'})
        invoke({**begin, 'bundle': {**bundle, 'model_fingerprint': '0' * 64}}, failure=True)
        invoke({'op': 'build', 'generation': 1}, failure=True)
        fill = {'op': 'backfill', 'generation': 1, 'waves': 1}
        invoke(fill, kill='before-backfill-commit')
        assert invoke(begin)['base']['completed'] == 0
        invoke(fill, kill='after-backfill-commit')
        assert invoke(begin)['base']['completed'] == 8
        change = {'request_id': 'corpus-move', 'memory': 1_000_000, 'expected': 0, 'scope': 7, 'active': True, 'body': rows[0]['body']}
        assert invoke({'op': 'commit', 'change': change})['commit_seq'] == 1
        invoke({'op': 'raw', 'cypher': "MATCH (d:Document {memory:1000024}) SET d.body='corrupt body length'"})
        invoke({**fill, 'waves': 3}, failure=True)
        assert invoke(begin)['base']['completed'] == 24
        invoke({'op': 'raw', 'cypher': "MATCH (d:Document {memory:1000024}) SET d.body='Synthetic backfill diagnostic only 16'"})
        complete = invoke({**fill, 'waves': 3})
        assert complete['base']['completed'] == 25 and complete['base']['complete']
        assert complete['base']['copied_vectors'] == 8
        assert invoke(fill)['base'] == complete['base']
        count = invoke({'op': 'raw', 'cypher': 'MATCH (d:Derived1Text) RETURN count(d)'})
        assert count['rows'] == [[25]]
        invoke({'op': 'build', 'generation': 1})
        invoke({'op': 'activate', 'generation': 1}, failure=True)
        vector = {'model': bundle['model'], 'model_fingerprint': fingerprint, 'body_sha256': hashlib.sha256(rows[0]['body'].encode()).hexdigest(), 'values': rows[0]['embedding']}
        for family in ['text', 'vector']:
            invoke({'op': 'apply', 'generation': 1, 'family': family, 'seq': 1, 'vector': vector if family == 'vector' else None})
        invoke({'op': 'activate', 'generation': 1})
        for scope in range(8):
            invoke({'op': 'grant', 'principal': 'reader', 'scope': scope, 'allowed': True})
        query = {'op': 'query', 'generation': 1, 'bundle': bundle, 'principal': 'reader', 'scope': 7, 'vector': rows[0]['embedding']}
        source_query = {**query, 'scope': int(rows[1]['partition'][1]), 'vector': rows[1]['embedding']}
        invoke({'op': 'raw', 'cypher': 'MATCH (d:Derived1Vector {memory:1000001}) SET d.memory=1000099'})
        assert all(hit['memory'] != 1_000_099 for hit in invoke(source_query)['hits'])
        invoke({'op': 'raw', 'cypher': 'MATCH (d:Derived1Vector {memory:1000099}) SET d.memory=1000001'})
        wrong_scope = (int(rows[1]['partition'][1]) + 1) % 8
        invoke({'op': 'raw', 'cypher': f"MATCH (d:Derived1Vector {{memory:1000001}}) SET d.partition='p{wrong_scope}'"})
        assert all(hit['memory'] != 1_000_001 for hit in invoke({**source_query, 'scope': wrong_scope})['hits'])
        invoke({'op': 'raw', 'cypher': f"MATCH (d:Derived1Vector {{memory:1000001}}) SET d.partition='{rows[1]['partition']}'"})
        invoke({'op': 'raw', 'cypher': 'MATCH (d:Document {memory:1000002}) SET d.memory=1000098'})
        invoke({**query, 'scope': int(rows[2]['partition'][1]), 'vector': rows[2]['embedding']}, failure=True)
        invoke({'op': 'raw', 'cypher': 'MATCH (d:Document {memory:1000098}) SET d.memory=1000002'})
        def original(i):
            return {'memory': 1_000_000 + i, 'seq': 0, 'scope': int(rows[i]['partition'][1]), 'active': True, 'body': rows[i]['body']}
        for i in range(25):
            expected = {**original(i), **({'seq': 1, 'scope': 7} if i == 0 else {})}
            assert invoke({'op': 'current', 'principal': 'reader', 'memory': 1_000_000 + i})['entry'] == expected
        for i in range(8):
            assert invoke({'op': 'history', 'principal': 'reader', 'memory': 1_000_000 + i, 'at': 0, 'scope': int(rows[i]['partition'][1])})['entry'] == original(i)
        for at, scope in [(0, 0), (1, 7)]:
            expected = [{**original(i), **({'seq': 1, 'scope': 7} if i == 0 and at else {})} for i in range(25)]
            expected = [row for row in expected if row['scope'] == scope]
            assert invoke({'op': 'state', 'principal': 'reader', 'scope': scope, 'at': at, 'limit': 100})['entries'] == expected
        for limit in [7, 8, 9]:
            cursor, collected = None, []
            while True:
                page = invoke({'op': 'state', 'principal': 'reader', 'scope': 0, 'at': 0, 'cursor': cursor, 'limit': limit})
                collected.extend(page['entries'])
                if page['next'] is None:
                    break
                assert cursor is None or page['next'] > cursor
                cursor = page['next']
            assert collected == [original(i) for i in range(25) if rows[i]['partition'] == 'p0']
        history = {'op': 'history', 'principal': 'reader', 'memory': 1_000_000, 'at': 0, 'scope': int(rows[0]['partition'][1])}
        current = {'op': 'current', 'principal': 'reader', 'memory': 1_000_000}
        invoke({'op': 'grant', 'principal': 'reader', 'scope': history['scope'], 'allowed': False})
        assert invoke(history)['entry'] is None
        assert invoke(current)['entry']['seq'] == 1
        invoke({'op': 'grant', 'principal': 'reader', 'scope': 7, 'allowed': False})
        assert invoke(current)['entry'] is None
        invoke({'op': 'grant', 'principal': 'reader', 'scope': history['scope'], 'allowed': True})
        assert invoke(history)['entry'] == original(0)
        invoke({'op': 'grant', 'principal': 'reader', 'scope': 7, 'allowed': True})
        assert invoke({**current, 'principal': 'unknown'})['entry'] is None
        copied = invoke({'op': 'raw', 'cypher': 'MATCH (d:Derived1Text) RETURN d.memory,d.body ORDER BY d.memory'})
        assert copied['rows'] == [[1_000_000 + i, row['body']] for i, row in enumerate(rows)]
        lexical = invoke({**query, 'scope': 0, 'vector': None, 'term': 'backfill'})
        assert any(hit['memory'] == 1_000_024 and hit['seq'] == 0 for hit in lexical['hits'])
        assert invoke(query)['hits'][0]['seq'] == 1
        assert all(hit['memory'] != 1_000_000 for hit in invoke({**query, 'scope': int(rows[0]['partition'][1])})['hits'])
        for i in range(1, 8):
            result = invoke({**query, 'scope': int(rows[i]['partition'][1]), 'vector': rows[i]['embedding']})
            assert result['hits'][0]['memory'] == 1_000_000 + i and result['hits'][0]['seq'] == 0
        second = {**begin, 'generation': 2, 'bundle': {**bundle, 'schema': 2}}
        invoke(second)
        invoke({'op': 'backfill', 'generation': 2, 'waves': 1})
        invoke({'op': 'raw', 'cypher': 'MATCH (d:Document {memory:1000008}) SET d.memory=999999'})
        invoke({'op': 'backfill', 'generation': 2, 'waves': 1}, failure=True)
        assert invoke(second)['base']['completed'] == 8
        invoke({'op': 'raw', 'cypher': 'MATCH (d:Document {memory:999999}) SET d.memory=1000008'})
        invoke({'op': 'backfill', 'generation': 2, 'waves': 3})
        invoke({'op': 'build', 'generation': 2})
        for family in ['text', 'vector']:
            invoke({'op': 'apply', 'generation': 2, 'family': family, 'seq': 1, 'vector': vector if family == 'vector' else None})
        invoke({'op': 'activate', 'generation': 2})
        assert invoke({**query, 'generation': 2, 'bundle': second['bundle']})['hits'][0]['seq'] == 1
        assert invoke(query)['hits'][0]['seq'] == 1
        for seq, active, scope in [(2, False, 7), (3, True, 6)]:
            mutation = {**change, 'request_id': f'corpus-restore-{seq}', 'expected': seq-1, 'active': active, 'scope': scope}
            assert invoke({'op': 'commit', 'change': mutation})['commit_seq'] == seq
            for family in ['text', 'vector']:
                invoke({'op': 'apply', 'generation': 2, 'family': family, 'seq': seq, 'vector': vector if family == 'vector' and active else None})
            if not active:
                assert all(hit['memory'] != 1_000_000 for hit in invoke({**query, 'generation': 2, 'bundle': second['bundle']})['hits'])
        restored = {**query, 'generation': 2, 'bundle': second['bundle'], 'scope': 6}
        lexical_restored = {**restored, 'vector': None, 'term': re.findall(r'\b[a-z]{8,24}\b', rows[0]['body'].lower())[0]}
        for request in [restored, lexical_restored]:
            hits = invoke(request)['hits']
            assert any(hit['memory'] == 1_000_000 and hit['seq'] == 3 and hit['source_key'] == rows[0]['key'] for hit in hits), hits
        for family in ['Vector', 'Text']:
            invoke({'op': 'raw', 'cypher': f"MATCH (d:Derived2{family} {{memory:1000000}}) SET d.source_key='forged-source'"})
        for request in [restored, lexical_restored]:
            assert any(hit['memory'] == 1_000_000 and hit['source_key'] == rows[0]['key'] for hit in invoke(request)['hits'])
        assert all(hit['memory'] != 1_000_000 for hit in invoke({**restored, 'scope': 7})['hits'])
        invoke({'op': 'grant', 'principal': 'reader', 'scope': 6, 'allowed': False})
        assert invoke(restored)['hits'] == []
        invoke({'op': 'grant', 'principal': 'reader', 'scope': 6, 'allowed': True})
        assert invoke(history)['entry'] == original(0)
        changed_manifest = output / 'changed-manifest.json'
        changed_manifest.write_text(manifest.read_text() + '\n')
        invoke({**second, 'manifest': str(changed_manifest)}, failure=True)
        final = invoke({'op': 'status'})
    assert not pathlib.Path(admission['temporary_root']).exists()
    assert len(records) == planned_commands
    assert digest(binary) == admission['binary_sha256']
    assert digest(pathlib.Path(__file__)) == admission['harness_sha256']
    publish(output / 'progress.json', {'phase': 'completed', 'completed': len(records), 'total': planned_commands, 'elapsed_s': time.monotonic() - started, 'rate_per_s': len(records) / (time.monotonic() - started), 'eta_s': 0})
    result = {'status': 'passed', 'seconds': time.monotonic() - started, 'binary_sha256': digest(binary), 'records': records, 'final': final, 'temporary_root_removed': True, 'not_tested': ['full corpus', 'sustained concurrent build', 'space admission', 'RSS sampling', 'power loss']}
    result['artifact_sha256'] = {p.name: digest(p) for p in sorted(output.iterdir()) if p.is_file()}
    publish(output / 'result.json', result)
    print(json.dumps({'status': 'passed', 'steps': len(records), 'seconds': result['seconds']}))


if __name__ == '__main__':
    main()

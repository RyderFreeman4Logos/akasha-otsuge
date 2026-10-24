#!/usr/bin/env python3
"""Run real generation searches and durable text-only indexing under 100 clients."""
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

from run_case import digest, process_identity, publish, verify_requests


def main():
    binary, source = [pathlib.Path(p).resolve(strict=True) for p in sys.argv[1:3]]
    output = pathlib.Path(sys.argv[3]).resolve()
    backend = sys.argv[4] if len(sys.argv) >= 5 else 'helix'
    rebuild = len(sys.argv) == 6
    assert backend in {'helix', 'fallback'} and len(sys.argv) in {4, 5, 6}
    assert not rebuild or (backend == 'fallback' and sys.argv[5] == '--rebuild')
    total = 11 if rebuild else 9
    output.mkdir(mode=0o700)
    (output / 'OWNER').write_text('akasha-otsuge MVP-003 generation workload receipts\n')
    assert source.stat().st_size < 4 * 1024**2
    rows = [json.loads(line) for line in source.read_text().splitlines()]
    assert len(rows) == 8 and {r['partition'] for r in rows} == {f'p{i}' for i in range(8)}
    rows += [dict(key=f'fixture:text:{i}', partition=f'p{i}', body=f'Synthetic unvectorized diagnostic {i}; excluded from corpus scale.') for i in range(8)]
    data = b''.join((json.dumps(row, separators=(',', ':')) + '\n').encode() for row in rows)
    prepared = output / 'input.jsonl'
    prepared.write_bytes(data)
    identity = dict(model='text-embedding-3-large', repository='Qdrant/dbpedia-entities-openai3-text-embedding-3-large-1536-1M', revision='4a9731217921bc476a0f03544f11f22ae4903fa5', precomputed_at='2024-02', precision='float32')
    fingerprint = hashlib.sha256(json.dumps(identity, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    bundle = dict(model=identity['model'], model_fingerprint=fingerprint, dimension=1536, chunker='source-document-v1', schema=1)
    manifest = output / 'manifest.json'
    manifest.write_text(json.dumps(dict(dimensions=1536, embedding_model=identity['model'], precomputed_at='2024-02 (publisher)', chunk_bytes=65536, synthetic_text_documents=8, corpus=dict(documents=len(rows), vectors=8, prepared_bytes=len(data), prepared_sha256=hashlib.sha256(data).hexdigest(), indexed_body_utf8_bytes=sum(len(r['body'].encode()) for r in rows), inputs=[dict(file='vectors-00000.parquet', repository=identity['repository'], revision=identity['revision'])]))))
    fixture = output / 'queries.json'
    fixture.write_text(json.dumps(dict(source_sha256=digest(manifest), corpus_manifest_sha256=digest(manifest), model=identity['model'], dimension=1536, queries=[dict(key=r['key'], partition=r['partition'], term=max(re.findall(r'[A-Za-z]{4,}', r['body']), key=len), memory=1_000_000+i, body_sha256=hashlib.sha256(r['body'].encode()).hexdigest(), embedding=r.get('embedding', []), text_only='embedding' not in r) for i, r in enumerate(rows)])))
    records, units = [], []
    started = time.monotonic()
    temporary = pathlib.Path(tempfile.mkdtemp(prefix='akasha-generation-workload-'))
    (temporary / 'OWNER').write_text('akasha-otsuge MVP-003 owned corpus import and database\n' if backend == 'fallback' else 'akasha-otsuge MVP-003 disposable generation workload database\n')
    database = temporary / 'database'
    observer = pathlib.Path(__file__).resolve().parents[1] / 'measure.py'
    admission = dict(binary_sha256=digest(binary), harness_sha256=digest(__file__), source_sha256=digest(source), temporary_root=str(temporary), controller=process_identity(os.getpid()))
    publish(output / 'admission.json', admission)

    def invoke(name, args):
        unit = 'akasha-gen-work-' + uuid.uuid4().hex
        units.append(unit)
        command = ['systemd-run', '--user', '--quiet', '--wait', '--pipe', '--collect', '--unit='+unit, '-p', 'MemoryMax=671088640', '-p', 'MemorySwapMax=0', '-p', 'RuntimeMaxSec=7200', '-p', 'Environment=AKASHA_SPIKE_PROFILE=low', '-p', 'Environment=AKASHA_SPIKE_DISK_RESERVE_BYTES=36212254720', sys.executable, str(observer), str(output / (name+'-memory.jsonl')), str(binary), str(database), *args]
        if backend == 'fallback':
            position = command.index(sys.executable)
            command[position:position] = ['-p', 'LimitNOFILE=8192', '-p', 'Environment=LANCE_CPU_THREADS=2',
                '-p', 'Environment=LANCE_DEFAULT_IO_BUFFER_SIZE=16777216', '-p', 'Environment=LANCE_SHUFFLE_BATCH_BYTES=16777216',
                '-p', 'Environment=LANCE_SHUFFLE_MAX_PRELOADED_OFFSETS_BYTES=0']
        publish(output / 'progress.json', dict(phase=name, completed=len(records), total=total, elapsed_s=time.monotonic()-started, child_progress=str(database / 'generation-1-catch-up.json')))
        with (output / (name+'.log')).open('x') as log:
            terminal = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT)
        record = dict(name=name, command=command, exit=terminal.returncode)
        records.append(record)
        publish(output / 'commands.json', records)
        assert terminal.returncode == 0, (name, terminal.returncode)
        return json.loads((output / (name+'.log')).read_text().splitlines()[-1])

    def request(name, value):
        path = output / (name+'.json')
        path.write_text(json.dumps(value))
        return invoke(name, ['--generation', str(path)])

    try:
        invoke('load', ['--load', str(prepared), str(manifest)])
        invoke('seed-text', ['--oracle-seed-text', '4096'])
        request('begin', dict(op='begin_corpus', generation=1, bundle=bundle, manifest=str(manifest)))
        request('backfill', dict(op='backfill', generation=1, waves=2))
        request('build', dict(op='build', generation=1))
        caught = request('catch-up', dict(op='catch_up', generation=1, waves=4096))
        if backend == 'fallback':
            assert caught['text']['watermark'] == caught['vector']['watermark'] == 4096 and caught['done']
        else:
            assert caught['text'] == caught['vector'] == 4096 and caught['remaining'] == 0
        request('activate', dict(op='activate', generation=1))
        invoke('verify-text', ['--oracle-verify-text', '4096'])
        if rebuild:
            request('begin-next', dict(op='begin_corpus', generation=2, bundle={**bundle, 'schema': 2}, manifest=str(manifest)))
            request('backfill-next', dict(op='backfill', generation=2, waves=2))
        rebuild_setting = 'rebuild_generation = 2\n' if rebuild else ''
        config = output / 'case.toml'
        config.write_text(f'''[run]
id = "generation-work-{uuid.uuid4().hex[:12]}"
profile = "low"
phase = "steady"
seconds = 60
diagnostic = true
generation = 1
{rebuild_setting}[paths]
binary = {json.dumps(str(binary))}
database = {json.dumps(str(database))}
fixture = {json.dumps(str(fixture))}
output = {json.dumps(str(output / 'case'))}
corpus_manifest = {json.dumps(str(manifest))}
[disk]
start_bytes = 36212254720
reserve_bytes = 36212254720
[reliability]
max_attempts = 1
''')
        with (output / 'case-controller.log').open('x') as log:
            terminal = subprocess.run([sys.executable, str(pathlib.Path(__file__).with_name('run_case.py')), str(config)], stdout=log, stderr=subprocess.STDOUT)
        state = json.loads((output / 'case/state.json').read_text())
        units.append(state['unit'])
        assert terminal.returncode == 0, ('case', terminal.returncode)
        oracle = state['result']['content_oracle']
        assert oracle['text_only_current_checks'] > 0 and oracle['text_only_bm25_checks'] > 0
        assert oracle['text_only_current_partitions'] == [f'p{i}' for i in range(8)]
        assert oracle['text_only_bm25_partitions'] == [f'p{i}' for i in range(8)]
        assert oracle['search_checks']['ann'] > 0
        derived = state['result']['engine']['snapshot']['derived']
        assert derived['remaining'] == 0 and derived['text'] == derived['vector'] == derived['committed']
        rebuild_controls = []
        if rebuild:
            path = output / 'case/runtime/rebuild.json'
            original = path.read_bytes()
            report = json.loads(original)
            assert report['status'] == 'passed' and report['ready']
            for patch in [dict(generation=1), dict(active_generation=2), dict(ready=False),
                          dict(start_seq=report['end_seq']), dict(end_seq=report['start_seq']),
                          dict(start_seq=0), dict(status='failed', error='')]:
                try:
                    path.write_text(json.dumps({**report, **patch}))
                    try:
                        verify_requests(output / 'case')
                    except ValueError as error:
                        rebuild_controls.append(dict(mutation=patch, rejection=str(error)))
                    else:
                        raise AssertionError(('rebuild evidence mutation accepted', patch))
                finally:
                    path.write_bytes(original)
            assert digest(path) == state['result']['artifact_sha256']['runtime/rebuild.json']
            requests = output / 'case/runtime/requests.jsonl'
            original_requests = requests.read_bytes()
            try:
                rows = [json.loads(line) for line in original_requests.splitlines()]
                wrong = next(row for row in rows if row['status'] == 'success' and row['operation'] in {'bm25', 'ann'})
                wrong['receipt']['validation']['generation'] = 2
                requests.write_text(''.join(json.dumps(row) + '\n' for row in rows))
                try:
                    verify_requests(output / 'case')
                except ValueError as error:
                    rebuild_controls.append(dict(mutation='search claims candidate generation', rejection=str(error)))
                else:
                    raise AssertionError('foreign-generation search receipt accepted')
            finally:
                requests.write_bytes(original_requests)
            assert digest(requests) == state['result']['artifact_sha256']['runtime/requests.jsonl']
        publish(output / 'result.json', dict(status='passed_diagnostic_only', binary_sha256=admission['binary_sha256'], commands=records, case=state['result'], rebuild_controls=rebuild_controls, seconds=time.monotonic()-started, not_tested=['full scale', '30-minute profiles', 'corpus corrections'] + ([] if rebuild else ['concurrent rebuild'])))
        publish(output / 'progress.json', dict(phase='completed', completed=total, total=total, elapsed_s=time.monotonic()-started, eta_s=0))
        print(json.dumps(dict(status='passed', seconds=time.monotonic()-started)))
    finally:
        states = [subprocess.run(['systemctl', '--user', 'show', unit, '-p', 'ActiveState', '--value'], check=True, capture_output=True, text=True).stdout.strip() for unit in units]
        if all(state in {'inactive', 'failed'} for state in states):
            # Failed fixtures remain available for bounded diagnosis until explicitly collected.
            if (output / 'result.json').exists():
                shutil.rmtree(temporary)


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Select lexical/current queries by replaying the frozen deduplicated source order."""
import argparse
import fcntl
import hashlib
import json
import os
import pathlib
import re
import sys
import time

import pyarrow

from download import publish
from prepare_corpus import records

OWNER = 'akasha-otsuge MVP-003 owned text query preparation\n'
CONFIG = '[execution]\nqueries = 512\nconfig_epoch = 1\n[reliability]\nmax_attempts = 1\n'


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def sample(record, ordinal):
    if 'embedding' in record:
        return None
    body = record['body']
    body_hash = hashlib.sha256(body.encode()).hexdigest()
    if record['key'] != 'fineweb:' + body_hash:
        raise ValueError('text identity differs from its source content')
    terms = sorted(set(re.findall(r'\b[a-z]{8,24}\b', body.lower())), key=lambda term: (-len(term), term))
    if not terms:
        return None
    return dict(key=record['key'], partition=record['partition'], term=terms[0],
                memory=1_000_000+ordinal, body_sha256=body_hash, text_only=True)


def reconcile(selected, index, query):
    if index < len(selected):
        if selected[index] != query:
            raise ValueError('completed text selection changed during source replay')
        return False
    if index != len(selected):
        raise ValueError('text selection journal has a gap')
    selected.append(query)
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['input', 'manifest', 'output']:
        parser.add_argument(name, type=pathlib.Path)
    parser.add_argument('--resume', action='store_true')
    args = parser.parse_args()
    inputs, manifest_path, output = args.input.resolve(strict=True), args.manifest.resolve(strict=True), args.output.resolve()
    with manifest_path.open('rb') as source:
        manifest_bytes = source.read(1024**2 + 1)
    if len(manifest_bytes) > 1024**2:
        raise ValueError('manifest exceeds 1 MiB')
    manifest = json.loads(manifest_bytes)
    code = {name: digest(pathlib.Path(__file__).with_name(name)) for name in ['prepare_corpus.py', 'download.py', 'sources.json']}
    environment = dict(python=sys.version.split()[0], pyarrow=pyarrow.__version__)
    if manifest['code_sha256'] != code or manifest['environment'] != environment:
        raise ValueError('source preparer or environment changed')
    total, vectors = manifest['corpus']['documents'], manifest['corpus']['vectors']
    if total - vectors < 512:
        raise ValueError('full lexical population requires at least 512 unvectorized documents')
    identity = dict(input=str(inputs), manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(),
                    source_code_sha256=code, script_sha256=digest(pathlib.Path(__file__)), environment=environment,
                    config_sha256=hashlib.sha256(CONFIG.encode()).hexdigest(), config_epoch=1)
    if args.resume:
        if (output/'OWNER').read_text() != OWNER:
            raise ValueError('foreign text preparation directory')
    else:
        output.mkdir(mode=0o700)
        (output/'OWNER').write_text(OWNER)
        (output/'run.toml').write_text(CONFIG)
    lock = (output/'lock').open('a')
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    state_path = output/'state.json'
    state = json.loads(state_path.read_text()) if args.resume else dict(identity=identity, selected=[], phase='admitted')
    if state['identity'] != identity or digest(output/'run.toml') != identity['config_sha256']:
        raise ValueError('immutable text preparation identity changed')
    if state['phase'] == 'completed':
        if (digest(output/'text-queries.json') != state['result']['sha256']
                or json.loads((output/'text-queries.json').read_text())['queries'] != state['selected']):
            raise ValueError('completed text query artifact changed')
        print(json.dumps(state['result']))
        return
    prior_count, selected = len(state['selected']), state['selected']
    if prior_count > 512:
        raise ValueError('text selection journal exceeds its bound')
    start_ticks = pathlib.Path('/proc/self/stat').read_text().rsplit(')', 1)[1].split()[19]
    state.update(phase='verifying_source_prefix' if prior_count else 'selecting', pid=os.getpid(), start_ticks=start_ticks)
    publish(state_path, state)
    started, found, statistics = time.monotonic(), 0, {}
    for ordinal, record in enumerate(records(inputs, manifest['source_counts'], statistics, output/'source-progress.json')):
        if found < 512 and ordinal >= vectors + found * (total-vectors) // 512:
            query = sample(record, ordinal)
            if query is not None:
                if digest(output/'run.toml') != identity['config_sha256']:
                    raise ValueError('selection config changed; use a new immutable run')
                if reconcile(selected, found, query):
                    publish(state_path, state)
                found += 1
        if ordinal % 4096 == 0 or ordinal+1 == total:
            elapsed = time.monotonic()-started
            rate = (ordinal+1)/elapsed
            publish(output/'progress.json', dict(phase='verifying_source_prefix' if found < prior_count else 'selecting',
                completed=ordinal+1, total=total, selected=found, selection_total=512, elapsed_s=elapsed,
                rate_per_s=rate, eta_s=(total-ordinal-1)/rate, pid=os.getpid(), start_ticks=start_ticks, config_epoch=1))
    expected = {k:v for k,v in manifest['corpus'].items() if k not in {'prepared_bytes', 'prepared_sha256'}}
    if statistics != expected or found != len(selected) or found != 512 or {q['partition'] for q in selected} != {f'p{i}' for i in range(8)}:
        raise ValueError('replayed corpus or selected coverage differs from frozen manifest')
    result = dict(corpus_manifest_sha256=identity['manifest_sha256'], queries=selected,
                  selection='512 evenly spaced ordinal targets in the deduplicated text stream; first eligible ASCII term after each target',
                  verified='pinned source hashes, frozen preparer/environment, deduplication order and all corpus statistics',
                  not_proven=['BM25 rank', 'file-cache-cold', 'new prepared stream byte-hash reserialization'])
    publish(output/'text-queries.json', result)
    state.update(phase='completed', result=dict(queries=512, documents=total, sha256=digest(output/'text-queries.json'), elapsed_s=time.monotonic()-started))
    publish(state_path, state)
    progress = json.loads((output/'progress.json').read_text())
    publish(output/'progress.json', {**progress, 'phase':'completed', 'eta_s':0})
    print(json.dumps(state['result']))


if __name__ == '__main__':
    main()

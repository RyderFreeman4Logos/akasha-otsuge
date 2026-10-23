#!/usr/bin/env python3
"""Check real queries and reject corrupted sidecars before workload effects."""

import copy
import hashlib
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile


def main():
    binary, database, fixture, output = map(lambda value:pathlib.Path(value).absolute(), sys.argv[1:])
    original = json.loads(fixture.read_text())
    if output.exists():
        raise ValueError('output already exists')
    with tempfile.TemporaryDirectory(prefix='akasha-query-check-') as directory:
        directory = pathlib.Path(directory)
        metadata = directory / 'queries.json'
        vectors = directory / original['vector_file']['name']
        shutil.copyfile(fixture.parent / vectors.name, vectors)
        config = directory / 'request.json'
        config.write_text(json.dumps({'run_id':'negative-query','phase':'burst','seconds':1,'diagnostic':True,
            'fixture':str(metadata),'output':str(directory/'must-not-create-output'),'corpus_manifest':None}))
        checks = []
        for case in ['offset','path','dimension','hash','zero','nan','neighbors','missing_memory','duplicate_memory','body_hash']:
            value = copy.deepcopy(original)
            shutil.copyfile(fixture.parent / vectors.name, vectors)
            if case == 'offset':value['queries'][0]['vector_offset'] = 1
            if case == 'path':value['vector_file']['name'] = '../queries.f32'
            if case == 'dimension':value['dimension'] = 3
            if case == 'hash':value['vector_file']['sha256'] = '0'*64
            if case == 'neighbors':value['queries'][0]['exact_neighbors'] = ['wrong']
            if case == 'missing_memory':del value['queries'][0]['memory']
            if case == 'duplicate_memory':value['queries'][1]['memory'] = value['queries'][0]['memory']
            if case == 'body_hash':value['queries'][0]['body_sha256'] = 'z' * 64
            if case in {'zero','nan'}:
                with vectors.open('r+b') as stream:
                    stream.write(b'\0'*(1536*4) if case == 'zero' else b'\0\0\xc0\x7f')
                with vectors.open('rb') as stream:value['vector_file']['sha256'] = hashlib.file_digest(stream,'sha256').hexdigest()
            metadata.write_text(json.dumps(value))
            absent = directory / 'must-not-create-database'
            result = subprocess.run([str(binary),str(absent),'--workload',str(config)],capture_output=True,text=True,timeout=30)
            assert result.returncode != 0 and not absent.exists() and not (directory/'must-not-create-output').exists(), (case,result.stderr)
            checks.append(case)
        result = subprocess.run([str(binary),str(database),'--search-corpus',str(fixture)],capture_output=True,text=True,timeout=1200)
        if result.returncode:
            raise RuntimeError(result.stderr[-4000:])
        parsed = json.loads(result.stdout)
        output.write_text(json.dumps({'status':'passed','negative_controls':checks,'search':parsed},indent=2)+'\n')
        print(json.dumps({'status':'passed','negative_controls':checks,'executed_queries':parsed['queries'],
            'self_hits':{kind:sum(row['self_at_20'] for row in parsed['results'] if row['mode']==kind) for kind in ['bm25','ann']},
            'seconds':parsed['seconds'],'not_proven':['full corpus','full-corpus ANN recall','file-cache-cold']}))


if __name__ == '__main__':
    main()

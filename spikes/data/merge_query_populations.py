#!/usr/bin/env python3
"""Combine completed source-bound query populations without reprocessing corpus data."""
import hashlib
import json
import pathlib
import shutil
import sys

from download import publish


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def combine(vectors, text):
    identity = vectors.get('corpus_manifest_sha256')
    if (not isinstance(identity, str) or len(identity) != 64 or vectors.get('source_sha256') != identity
            or text.get('corpus_manifest_sha256') != identity or vectors.get('dimension') != 1536
            or vectors.get('model') != 'text-embedding-3-large'):
        raise ValueError('query populations do not share a pinned corpus/model')
    vq, tq = vectors['queries'], text['queries']
    if (len(tq) != 512 or not 8 <= len(vq) <= 3584
            or any(q.get('text_only') or q.get('vector_offset') != i*1536*4 or q.get('embedding') for i, q in enumerate(vq))
            or any(q.get('text_only') is not True or 'embedding' in q or 'vector_offset' in q
                   or q.get('key') != 'fineweb:'+q.get('body_sha256', '') for q in tq)
            or any({q['partition'] for q in group} != {f'p{i}' for i in range(8)} for group in [vq, tq])):
        raise ValueError('invalid vector/text query population coverage')
    queries = vq + tq
    if len({q['memory'] for q in queries}) != len(queries) or len({q['key'] for q in queries}) != len(queries):
        raise ValueError('query populations overlap')
    return {**vectors, 'queries': queries, 'selection': 'retained vector queries followed by 512 source-bound unvectorized queries'}


def main():
    vector_path, text_path, output = map(pathlib.Path, sys.argv[1:])
    values = []
    for path in [vector_path, text_path]:
        with path.open('rb') as source:
            data = source.read(4*1024**2+1)
        if len(data) > 4*1024**2:
            raise ValueError('query metadata exceeds 4 MiB')
        values.append(json.loads(data))
    result = combine(*values)
    sidecar = result['vector_file']
    if pathlib.Path(sidecar['name']).name != sidecar['name'] or sidecar['bytes'] != len(values[0]['queries'])*1536*4:
        raise ValueError('invalid vector sidecar')
    source = vector_path.parent / sidecar['name']
    if source.stat().st_size != sidecar['bytes'] or digest(source) != sidecar['sha256']:
        raise ValueError('source vector sidecar changed')
    output.mkdir(mode=0o700)
    (output/'OWNER').write_text('akasha-otsuge MVP-003 owned mixed query population\n')
    shutil.copyfile(source, output/sidecar['name'])
    if digest(output/sidecar['name']) != sidecar['sha256']:
        raise ValueError('copied vector sidecar changed')
    publish(output/'queries.json', result)
    publish(output/'result.json', dict(queries=len(result['queries']), text_queries=512,
        queries_sha256=digest(output/'queries.json'), vectors_sha256=sidecar['sha256'],
        input_sha256={str(p):digest(p) for p in [vector_path,text_path]}, script_sha256=digest(pathlib.Path(__file__))))
    print((output/'result.json').read_text())


if __name__ == '__main__':
    main()

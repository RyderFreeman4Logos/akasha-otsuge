#!/usr/bin/env python3
"""Compute exact cosine neighbors for frozen queries against every source vector."""

import argparse
import fcntl
import hashlib
import json
import os
import pathlib
import shutil
import struct
import subprocess
import sys
import time
import uuid

import pyarrow as pa
import pyarrow.compute as pc
import pyarrow.parquet as parquet

from download import publish, verify
from prepare_query_population import digest

OWNER = 'akasha-otsuge MVP-003 owned exact ANN oracle\n'
CONFIG = '[execution]\nqueries_per_partition = 4\nconfig_epoch = 1\n[reliability]\nmax_attempts = 1\n'


def cleanup(output, state):
    if not state['result']['temporary_corpus_removed']:
        for index, unit in state['units'].items():
            path = output / f'unit-{int(index)}.bin'
            if path.exists():
                if digest(path) != unit['sha256']:
                    raise ValueError('owned temporary vector corpus changed; inspect before cleanup')
                path.unlink()
        state['result']['temporary_corpus_removed'] = True
        publish(output/'state.json',state)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['input','manifest','queries','binary','output']:
        parser.add_argument(name,type=pathlib.Path)
    parser.add_argument('--resume',action='store_true')
    args = parser.parse_args()
    inputs, manifest_path, queries_path, binary, output = (getattr(args,key).absolute() for key in ['input','manifest','queries','binary','output'])
    if sys.byteorder != 'little' or manifest_path.stat().st_size>1024**2 or queries_path.stat().st_size>4*1024**2:
        raise ValueError('unsupported host or oversized manifest')
    manifest, population = json.loads(manifest_path.read_text()), json.loads(queries_path.read_text())
    inventory_path = pathlib.Path(__file__).with_name('sources.json')
    if digest(inventory_path)!=manifest['code_sha256']['sources.json'] or digest(manifest_path)!=population['corpus_manifest_sha256']:
        raise ValueError('corpus/query identity mismatch')
    sources = json.loads(inventory_path.read_text())['sources'][0]['files'][:manifest['source_counts'][0]]
    sidecar = population['vector_file']
    if pathlib.Path(sidecar['name']).name!=sidecar['name']:
        raise ValueError('invalid vector sidecar path')
    vector_path = queries_path.parent / sidecar['name']
    if digest(vector_path)!=sidecar['sha256'] or vector_path.stat().st_size!=sidecar['bytes']:
        raise ValueError('query vector sidecar changed')
    identity = {'manifest_sha256':digest(manifest_path),'population_sha256':digest(queries_path),
        'binary_sha256':digest(binary),'script_sha256':digest(pathlib.Path(__file__)),
        'oracle_source_sha256':digest(pathlib.Path(__file__).with_name('ann_oracle.rs')),
        'helper_sha256':{name:digest(pathlib.Path(__file__).with_name(name)) for name in ['prepare_query_population.py','download.py']},
        'config_sha256':hashlib.sha256(CONFIG.encode()).hexdigest(),
        'input':str(inputs),'selection':'sha256(ann-holdout-v1:key), first four per partition',
        'profile':'offline preparation, not engine acceptance','pyarrow':pa.__version__}
    if args.resume:
        if (output/'OWNER').read_text()!=OWNER:raise ValueError('foreign oracle output')
    else:
        if shutil.disk_usage(output.parent).free < 22*1024**3:raise ValueError('need scratch plus 15 GiB free reserve')
        output.mkdir(mode=0o700);(output/'OWNER').write_text(OWNER)
        (output/'run.toml').write_text(CONFIG)
    lock = (output/'lock').open('a');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
    state_path = output/'state.json'
    state = json.loads(state_path.read_text()) if args.resume else {'identity':identity,'units':{},'attempts':[]}
    if state['identity']!=identity or digest(output/'run.toml')!=identity['config_sha256']:raise ValueError('oracle run identity changed')
    if state.get('phase')=='completed':
        for name,expected in state['result']['artifact_sha256'].items():
            if digest(output/name)!=expected:raise ValueError('completed oracle changed')
        cleanup(output,state)
        print(json.dumps(state['result']));return
    selected=[]
    for partition in range(8):
        group=[q for q in population['queries'] if q['partition']==f'p{partition}']
        selected.extend(sorted(group,key=lambda q:hashlib.sha256(('ann-holdout-v1:'+q['key']).encode()).digest())[:4])
    if len(selected)!=32:raise ValueError('oracle needs four queries per partition')
    with vector_path.open('rb') as source, (output/'query-input.bin').open('wb') as target, (output/'oracle-vectors.f32').open('wb') as vectors:
        target.write(struct.pack('<I',len(selected)))
        for index,query in enumerate(selected):
            source.seek(query['vector_offset']);data=source.read(1536*4)
            if len(data)!=1536*4:raise ValueError('truncated query vector')
            key=query['key'].encode();target.write(struct.pack('<BH',int(query['partition'][1]),len(key))+key+data);vectors.write(data)
            query['vector_offset']=index*1536*4
        for stream in [target,vectors]:stream.flush();os.fsync(stream.fileno())
    state.update(phase='preparing',pid=os.getpid(),start_ticks=pathlib.Path('/proc/self/stat').read_text().rsplit(')',1)[1].split()[19])
    publish(state_path,state);started=time.monotonic();prior_units=len(state['units'])
    for index,item in enumerate(sources):
        path=output/f'unit-{index}.bin'
        if str(index) in state['units']:
            if digest(path)!=state['units'][str(index)]['sha256']:raise ValueError('completed vector unit changed')
            continue
        if shutil.disk_usage(output).free < 16*1024**3:raise ValueError('oracle scratch reservation reached')
        elapsed=time.monotonic()-started;rate=max(0,index-prior_units)/elapsed
        publish(output/'prepare-progress.json',{'phase':'prepare_vector_stream','completed':index,'total':len(sources),
            'elapsed_s':elapsed,'rate_per_s':rate,'eta_s':(len(sources)-index)/rate if rate else None,'pid':state['pid'],'start_ticks':state['start_ticks']})
        source=inputs/f'vectors-{index:05d}.parquet';verify(source,item)
        table=parquet.ParquetFile(source);temporary=path.with_suffix('.tmp')
        with temporary.open('wb') as target:
            target.write(struct.pack('<I',table.metadata.num_rows));written=0
            for batch in table.iter_batches(batch_size=1024,columns=['_id','text-embedding-3-large-1536-embedding']):
                column=batch.column(1)
                if column.null_count or pc.min_max(pc.list_value_length(column)).as_py()!={'min':1536,'max':1536}:
                    raise ValueError('invalid source vector dimensions')
                flat=pc.list_flatten(column).cast(pa.float32())
                if flat.null_count:raise ValueError('null vector element')
                values=flat.buffers()[1].slice(flat.offset*4,len(flat)*4)
                for ordinal,key in enumerate(batch.column(0).to_pylist()):
                    key=('dbpedia:'+key).encode()
                    if not 0<len(key)<=256:raise ValueError('invalid source vector identity')
                    target.write(struct.pack('<BH',hashlib.sha256(key).digest()[0]%8,len(key))+key)
                    target.write(values.slice(ordinal*1536*4,1536*4));written+=1
            if written!=table.metadata.num_rows:raise ValueError('vector stream lost rows')
            target.flush();os.fsync(target.fileno())
        os.replace(temporary,path)
        state['units'][str(index)]={'sha256':digest(path),'rows':written,'source_sha256':item['sha256']};publish(state_path,state)
    if sum(unit['rows'] for unit in state['units'].values())!=manifest['corpus']['vectors']:raise ValueError('vector population count mismatch')
    attempt=uuid.uuid4().hex
    command=[str(binary),str(output/'query-input.bin'),str(output),str(len(sources)),str(manifest['corpus']['vectors']),str(output/'progress.json')]
    state.update(phase='exact_cosine');state['attempts'].append({'id':attempt,'command':command,'query_sha256':digest(output/'query-input.bin')});publish(state_path,state)
    with (output/(attempt+'.tsv')).open('x') as result, (output/(attempt+'.log')).open('x') as log:
        terminal=subprocess.run(command,stdout=result,stderr=log,pass_fds=(lock.fileno(),),timeout=43200)
    state['attempts'][-1]['exit_code']=terminal.returncode;publish(state_path,state)
    if terminal.returncode:raise RuntimeError('exact cosine process failed; preserve this attempt for explicit resume')
    neighbors=[[] for _ in selected]
    for line in (output/(attempt+'.tsv')).read_text().splitlines():
        query,rank,key,score=line.split('\t');query,rank=int(query),int(rank)
        if rank!=len(neighbors[query]):raise ValueError('oracle ranks are not contiguous')
        neighbors[query].append({'key':bytes.fromhex(key).decode(),'cosine':float(score)})
    for query,rows in zip(selected,neighbors):
        if len(rows)!=20 or len({row['key'] for row in rows})!=20 or query['key'] not in {row['key'] for row in rows}:raise ValueError('oracle lacks self or twenty unique neighbors')
        query['exact_neighbors']=[row['key'] for row in rows];query['neighbor_cosines']=[row['cosine'] for row in rows]
    result={'source_sha256':identity['manifest_sha256'],'corpus_manifest_sha256':identity['manifest_sha256'],
        'model':'text-embedding-3-large','dimension':1536,'queries':selected,
        'vector_file':{'name':'oracle-vectors.f32','bytes':(output/'oracle-vectors.f32').stat().st_size,'sha256':digest(output/'oracle-vectors.f32')},
        'oracle':{'method':'exact cosine over every source float32 vector, float64 accumulation; ties by key',
            'vectors':manifest['corpus']['vectors'],'identity':identity,'eligible_partition_filter':True}}
    publish(output/'queries.json',result)
    artifacts=['queries.json','oracle-vectors.f32','query-input.bin',attempt+'.tsv',attempt+'.log','progress.json']
    state.update(phase='completed',result={'status':'prepared','queries':32,'vectors':manifest['corpus']['vectors'],
        'artifact_sha256':{name:digest(output/name) for name in artifacts},'temporary_corpus_removed':False})
    publish(state_path,state)
    cleanup(output,state)
    print(json.dumps(state['result']))


if __name__=='__main__':
    main()

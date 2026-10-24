#!/usr/bin/env python3
"""Native outbox application, gap-free watermarks and atomic generation switches."""
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
    binary, source, output = [pathlib.Path(p).resolve() for p in sys.argv[1:]]
    rows = [json.loads(line) for line in source.read_text().splitlines()]
    output.mkdir(mode=0o700)
    (output/'OWNER').write_text('akasha-otsuge MVP-003 fallback lifecycle receipts\n')
    bundle = dict(model='text-embedding-3-large', dimension=1536, chunker='source-document-v1', schema=1,
        model_fingerprint='c87f6e6c52567e56980cb3281f8a7b893276b665b3dd95bf00cabe024657fea5')
    steps, snapshots, contenders = [], {}, []
    def add(channel, request, check=lambda _: True, *, error=None, kill=None, env=None):
        steps.append((channel, request, check, error, kill, env))
    def g(op, check=lambda _: True, generation=1, **fields):
        error, kill, env = fields.pop('error',None), fields.pop('kill',None), fields.pop('env',None)
        add('generation',dict(op=op,generation=generation,**fields),check,error=error,kill=kill,env=env)
    def commit(seq, memory, row, expected=0, active=True, text_only=False):
        change=dict(request_id=f'life:{seq}',memory=memory,expected=expected,active=active,
            scope=int(rows[row]['partition'][1]),body=rows[row]['body'],text_only=text_only)
        add('authority',dict(op='apply',change=change),lambda r:r['commit_seq']==seq)
    def vector(row):
        return dict(model=bundle['model'],model_fingerprint=bundle['model_fingerprint'],
            body_sha256=hashlib.sha256(rows[row]['body'].encode()).hexdigest(),values=rows[row]['embedding'])
    def apply(seq, family, row=None, generation=1, **extra):
        g('apply',generation=generation,family=family,seq=seq,vector=None if row is None else vector(row),**extra)
    def receipt(seq, check, generation=1):
        g('indexed_receipt',check,generation=generation,request_id=f'life:{seq}')
    def query(check, generation=1, text=False, row=0, **extra):
        clause=dict(term=max(__import__('re').findall(r'[A-Za-z]{4,}',rows[row]['body']),key=len)) if text else dict(vector=rows[row]['embedding'])
        g('query_current',check,generation=generation,bundle={**bundle,'schema':generation},
            principal='reader',scope=int(rows[row]['partition'][1]),**clause,**extra)
    def has(memory, seq):
        return lambda r:any(h['memory']==memory and h['seq']==seq for h in r['hits'])
    def prune(check=lambda _: True, generation=2, **fields):
        g('prune_versions',check,generation=generation,
            deadline_ms=fields.pop('deadline_ms',int(time.time()*1000)+600_000),**fields)
    retire_deadline=int(time.time()*1000)+600_000
    def retire(check=lambda _: True, generation=1, **fields):
        g('retire_generation',check,generation=generation,
            deadline_ms=fields.pop('deadline_ms',retire_deadline),**fields)
    def remember(name):
        def check(value):
            snapshots[name]=value
            return True
        return check
    add('load',{})
    retire(error='retirement requires an active generation')
    for scope in range(8):
        add('authority',dict(op='permission',principal='reader',scope=scope,allowed=True))
    for op, fields in [('begin',dict(bundle=bundle)),('backfill',dict(waves=10)),('build',{})]:g(op,**fields)
    commit(1,1,0)
    receipt(1,lambda r:r['stage']=='committed')
    for bad in [dict(model='wrong'),dict(model_fingerprint='0'*64),dict(body_sha256='0'*64),dict(values=[1.0])]:
        g('apply',family='vector',seq=1,vector={**vector(0),**bad},error='embedding bundle or source identity mismatch')
    apply(1,'vector',error='embedding output required')
    apply(1,'text',0,error='text job cannot consume vector output')
    apply(1,'text');apply(1,'vector',0)
    receipt(1,lambda r:r['stage']=='indexed' and r['watermarks']==dict(text=1,vector=1))
    g('activate');query(has(1,1));query(has(1,1),text=True)
    retire(error='cannot retire the active generation')
    prune(lambda r:sum(t['old_versions'] for t in r['tables'])>0,generation=1,
          env=dict(AKASHA_SPIKE_DISK_RESERVE_BYTES=str(2**64-1)))
    query(has(1,1));query(has(1,1),text=True)
    commit(2,2,1);commit(3,3,2)
    apply(3,'text',env=dict(LANCE_FTS_PARTITION_SIZE='2048'),error='incremental FTS requires')
    for family in ['text','vector']:apply(3,family,2 if family=='vector' else None)
    receipt(3,lambda r:r['watermarks']==dict(text=1,vector=1))
    for family in ['text','vector']:apply(2,family,1 if family=='vector' else None)
    receipt(3,lambda r:r['watermarks']==dict(text=3,vector=3))
    commit(4,1,3,1);commit(5,1,3,4,active=False)
    for seq in [4,5]:
        for family in ['text','vector']:apply(seq,family)
    receipt(4,lambda r:r['reasons']==dict(text='superseded',vector='superseded'))
    query(lambda r:all(h['memory']!=1 for h in r['hits']))
    commit(6,1,0,5)
    apply(6,'text',hold='before-delta-append',kill='before-delta-append')
    apply(6,'text',hold='after-delta-append',kill='after-delta-append')
    prune(generation=1,error='prune requires drained outbox')
    apply(6,'text',hold='after-index-application',kill='after-index-application')
    query(has(1,6),text=True)
    receipt(6,lambda r:r['stage']=='committed')
    apply(6,'text')
    apply(6,'vector',0,hold='before-watermark',kill='before-watermark')
    receipt(6,lambda r:r['watermarks']==dict(text=6,vector=5))
    g('catch_up',waves=1)
    apply(6,'vector',0);query(has(1,6))
    g('begin',generation=2,bundle={**bundle,'schema':2});g('backfill',generation=2,waves=10)
    g('build',generation=2,fail_after_text=True,error='injected build failure')
    query(has(1,6));query(has(1,6),text=True)
    commit(7,4,4)
    g('build',generation=2)
    g('activate',generation=2,error='candidate generation has incomplete coverage')
    for seq,row in [(1,None),(2,1),(3,2),(4,None),(5,None),(6,0),(7,4)]:
        for family in ['text','vector']:apply(seq,family,row if family=='vector' else None,generation=2)
    g('activate',generation=2,hold='before-activation',kill='before-activation')
    g('lifecycle_status',lambda r:r['active']==1,generation=2)
    g('activate',generation=2,hold='after-activation',kill='after-activation')
    g('lifecycle_status',lambda r:r['active']==2,generation=2)
    query(has(1,6),generation=2);query(has(1,6),generation=2,text=True);query(has(1,6))
    g('query_current',generation=2,bundle=bundle,principal='reader',scope=int(rows[0]['partition'][1]),vector=rows[0]['embedding'],error='query bundle mismatch')
    g('apply',generation=2,family='vector',seq=6,vector={**vector(0),'model':'wrong'},error='embedding bundle or source identity mismatch')
    commit(8,1,0,6,text_only=True)
    retire(error='retirement requires drained active outbox')
    apply(8,'vector',0,generation=2,error='text-only source cannot consume vector output')
    apply(8,'text',generation=2);apply(8,'vector',generation=2)
    receipt(8,lambda r:r['reasons']==dict(text='applied',vector='not_required'),generation=2)
    query(lambda r:all(h['memory']!=1 for h in r['hits']),generation=2)
    query(has(1,8),generation=2,text=True)
    add('authority',dict(op='permission',principal='reader',scope=int(rows[0]['partition'][1]),allowed=False))
    query(lambda r:r['hits']==[],generation=2,text=True)
    add('authority',dict(op='permission',principal='reader',scope=int(rows[0]['partition'][1]),allowed=True))
    for seq in range(9,13):commit(seq,1,0,seq-1,text_only=True)
    query(lambda r:False,generation=2,text=True,min_commit_seq=12,error='index_lag')
    g('catch_up',generation=2,waves=2)
    receipt(12,lambda r:r['watermarks']==dict(text=12,vector=12),generation=2)
    query(has(1,12),generation=2,text=True,min_commit_seq=12)
    g('lifecycle_status',remember('lifecycle'),generation=2)
    receipt(12,remember('receipt'),generation=2)
    query(remember('text'),generation=2,text=True)
    query(remember('vector'),generation=2,row=1)
    for at in [1,5,6,12]:
        add('authority',dict(op='get',memory=1,at=at),remember(f'history:{at}'))
    prune(generation=1,error='prune requires the active ready generation')
    prune(deadline_ms=0,error='original operation deadline expired')
    prune(deadline_ms=2**64-1,error='prune deadline exceeds observation budget')
    prune(hold='before-prune',kill='before-prune')
    prune(hold='after-prune-table',kill='after-prune-table')
    query(lambda r:r==snapshots['text'],generation=2,text=True)
    query(lambda r:r==snapshots['vector'],generation=2,row=1)
    prune(lambda r:sum(t['old_versions'] for t in r['tables'])>0
          and all(t['failed_deletes']==0 for t in r['tables']))
    prune(lambda r:all(t['old_versions']==t['bytes_removed']==t['failed_deletes']==0 for t in r['tables']))
    g('lifecycle_status',lambda r:r==snapshots['lifecycle'],generation=2)
    receipt(12,lambda r:r==snapshots['receipt'],generation=2)
    query(lambda r:r==snapshots['text'],generation=2,text=True)
    query(lambda r:r==snapshots['vector'],generation=2,row=1)
    for at in [1,5,6,12]:
        add('authority',dict(op='get',memory=1,at=at),lambda r,at=at:r==snapshots[f'history:{at}'])
    apply(7,'text',hold='before-delta-append',kill='before-delta-append')
    retire(error='retirement requires no pending wave')
    apply(7,'text');apply(7,'vector',4)
    receipt(7,remember('old-receipt'))
    add('authority',dict(op='receipt',request_id='life:7'),remember('authority-receipt'))
    retire(generation=2,error='cannot retire the active generation')
    retire(generation=999,error='missing generation')
    retire(deadline_ms=0,error='original operation deadline expired')
    retire(deadline_ms=2**64-1,error='retirement deadline exceeds observation budget')
    retire(hold='unknown',error='invalid retirement checkpoint')
    retire(hold='before-retire',kill='before-retire')
    query(has(2,2),row=1)
    retire(hold='after-retire-intent',kill='after-retire-intent')
    g('status',lambda r:r['retirement']==dict(deadline_ms=retire_deadline,completed=0))
    query(lambda _:False,row=1,error='generation retired')
    g('query',bundle=bundle,scope=7,term='baseline',error='generation retired')
    for op,fields in [('begin',dict(bundle=bundle)),('backfill',dict(waves=1)),('build',{}),
                      ('activate',{}),('catch_up',dict(waves=1)),('advance',dict(family='text')),
                      ('apply',dict(family='text',seq=7)),('lifecycle_status',{})]:
        g(op,**fields,error='generation retired')
    prune(generation=1,error='generation retired')
    receipt(7,lambda r:r=={**snapshots['old-receipt'],'generation_retired':True})
    retire(deadline_ms=retire_deadline+1,error='original retirement deadline mismatch')
    retire(hold='after-retire-table',kill='after-retire-table')
    g('status',lambda r:r['retirement']==dict(deadline_ms=retire_deadline,completed=0))
    query(lambda r:r==snapshots['text'],generation=2,text=True)
    query(lambda r:r==snapshots['vector'],generation=2,row=1)
    retire(remember('retired'),env=dict(AKASHA_SPIKE_DISK_RESERVE_BYTES=str(2**64-1)))
    retire(lambda r:r==snapshots['retired'] and r['retired'] and r['completed']==r['total']==4)
    g('begin',bundle=bundle,error='generation retired')
    g('lifecycle_status',lambda r:r==snapshots['lifecycle'],generation=2)
    receipt(12,lambda r:r==snapshots['receipt'],generation=2)
    add('authority',dict(op='receipt',request_id='life:7'),lambda r:r==snapshots['authority-receipt'])
    for at in [1,5,6,12]:
        add('authority',dict(op='get',memory=1,at=at),lambda r,at=at:r==snapshots[f'history:{at}'])
    for generation, phase in [(3,'empty'),(4,'pending-backfill'),(5,'failed-build')]:
        g('begin',generation=generation,bundle={**bundle,'schema':generation})
        if phase=='pending-backfill':
            g('backfill',generation=generation,waves=1,hold='after-text',kill='after-text')
        elif phase=='failed-build':
            g('backfill',generation=generation,waves=10)
            g('build',generation=generation,fail_after_text=True,error='injected build failure')
        g('status',remember('unready:'+str(generation)),generation=generation)
        retire(generation=generation,hold='after-retire-intent',kill='after-retire-intent')
        def tombstone(value,generation=generation):
            prior=snapshots['unready:'+str(generation)]
            return not value['ready'] and value=={**prior,'retirement':dict(deadline_ms=retire_deadline,completed=0)}
        g('status',tombstone,generation=generation)
        for op,fields in [('begin',dict(bundle={**bundle,'schema':generation})),('backfill',dict(waves=1)),('build',{})]:
            g(op,generation=generation,**fields,error='generation retired')
        retire(generation=generation,hold='after-retire-table',kill='after-retire-table')
        retire(lambda r:r['retired'] and r['completed']==r['total']==2,generation=generation)
        retire(lambda r:r['retired'] and r['completed']==r['total']==2,generation=generation)
        g('lifecycle_status',lambda r:r==snapshots['lifecycle'],generation=2)
        receipt(12,lambda r:r==snapshots['receipt'],generation=2)
        query(lambda r:r==snapshots['text'],generation=2,text=True)
        query(lambda r:r==snapshots['vector'],generation=2,row=1)
    records, started = [], time.monotonic()
    with tempfile.TemporaryDirectory(prefix='akasha-fallback-lifecycle-') as temporary:
        root=pathlib.Path(temporary);database=root/'database';request=root/'request.json'
        (root/'OWNER').write_text('akasha-otsuge MVP-003 owned corpus import and database\n')
        documents=[dict(key='baseline',body='baseline content',partition='p7'),*rows]
        raw=b''.join((json.dumps(row,separators=(',',':'))+'\n').encode() for row in documents)
        (root/'input.jsonl').write_bytes(raw)
        publish(root/'manifest.json',dict(corpus=dict(documents=len(documents),vectors=len(rows),prepared_bytes=len(raw),prepared_sha256=hashlib.sha256(raw).hexdigest(),indexed_body_utf8_bytes=sum(len(row['body'].encode()) for row in documents))))
        publish(output/'admission.json',dict(binary_sha256=digest(binary),source_sha256=digest(source),harness_sha256=digest(__file__),temporary_root=temporary,controller=process_identity(os.getpid()),planned_commands=len(steps)))
        for channel, value, check, error, kill, overrides in steps:
            name=f'{len(records):03d}';publish(request,value)
            args=['--load',str(root/'input.jsonl'),str(root/'manifest.json')] if channel=='load' else ['--'+channel,str(request)]
            command=[str(binary),str(database),*args]
            env=dict(os.environ,LANCE_FTS_NUM_SHARDS='1',LANCE_FTS_PARTITION_SIZE='32')
            env.update(overrides or {})
            protected={str(p.relative_to(database)):digest(p) for p in (database/'derived').rglob('*') if p.is_file()} if error and value.get('op') in {'prune_versions','retire_generation'} else None
            retained={str(p.relative_to(database)):digest(p) for p in (database/'derived').rglob('*') if p.is_file() and not p.relative_to(database/'derived').parts[0].startswith(f"g{value['generation']}_")} if value.get('op')=='retire_generation' else None
            child=subprocess.Popen(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env)
            identity,prefix=process_identity(child.pid),b''
            try:
                if kill:
                    with selectors.DefaultSelector() as selector:
                        selector.register(child.stderr,selectors.EVENT_READ);until=time.monotonic()+180
                        while time.monotonic()<until:
                            assert child.poll() is None,prefix
                            if selector.select(timeout=1):
                                prefix+=os.read(child.stderr.fileno(),4096)
                                if f'PROBE_CHECKPOINT:{kill}\n'.encode() in prefix:break
                        else:raise TimeoutError(kill)
                    if kill in {'before-prune','before-retire'}:
                        before={str(p.relative_to(database)):digest(p) for p in (database/'derived').rglob('*') if p.is_file()}
                        contender=subprocess.Popen(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env)
                        contender_identity=process_identity(contender.pid)
                        try:
                            other_out,other_err=contender.communicate(timeout=30)
                        finally:
                            if contender.poll() is None:contender.kill();contender.communicate(timeout=10)
                        (output/(kill+'-contender.stdout')).write_bytes(other_out)
                        (output/(kill+'-contender.stderr')).write_bytes(other_err)
                        assert contender.returncode==1 and b'already locked by another process' in other_err
                        assert process_identity(contender.pid) is None
                        assert before=={str(p.relative_to(database)):digest(p) for p in (database/'derived').rglob('*') if p.is_file()}
                        contenders.append(dict(command=command,child=contender_identity,exit=contender.returncode,derived_unchanged=True))
                    assert process_identity(child.pid)['start_ticks']==identity['start_ticks'];child.kill()
                stdout,stderr=child.communicate(timeout=180)
                (output/(name+'.stdout')).write_bytes(stdout);(output/(name+'.stderr')).write_bytes(prefix+stderr)
                assert child.returncode==(-9 if kill else 1 if error else 0),(name,stderr[-2000:])
                if error:assert error.encode() in stderr,(name,stderr[-2000:])
                if protected is not None:
                    assert protected=={str(p.relative_to(database)):digest(p) for p in (database/'derived').rglob('*') if p.is_file()}
                if retained is not None:
                    assert retained=={str(p.relative_to(database)):digest(p) for p in (database/'derived').rglob('*') if p.is_file() and not p.relative_to(database/'derived').parts[0].startswith(f"g{value['generation']}_")}
                if value.get('op')=='retire_generation' and not error:
                    if kill=='after-retire-table':assert not (database/'derived'/f"g{value['generation']}_text.lance").exists()
                    if not kill:
                        assert not any(p.name.startswith(f"g{value['generation']}_") for p in (database/'derived').iterdir())
                        progress=json.loads((database/f"generation-{value['generation']}-retire.json").read_text())
                        assert progress['completed']==progress['total']
                        assert abs(progress['rate_per_s']-(progress['completed']-progress['completed_at_start'])/max(progress['elapsed_s'],0.001))<1e-6
                if not error and not kill:assert check(json.loads(stdout)),(name,stdout)
                if channel=='generation' and value == dict(op='apply', generation=1, family='text', seq=2, vector=None):
                    progress=json.loads((database/'g1_delta_text-body_fts-progress.json').read_text())
                    publish(output/'incremental-text-progress.json',progress)
                    assert progress['stages']['tokenize_docs']['completed']==1, progress
                if channel=='generation' and value['op']=='catch_up' and not error and not kill:
                    progress=json.loads((database/f"generation-{value['generation']}-catch-up.json").read_text())
                    assert progress['completed']>progress['completed_at_start']
                    assert abs(progress['rate_per_s']-(progress['completed']-progress['completed_at_start'])/progress['elapsed_s'])<1e-6
            finally:
                if child.poll() is None:child.kill()
                child.communicate(timeout=10)
            assert process_identity(child.pid) is None
            records.append(dict(command=command,request=value,child=identity,exit=child.returncode,error=error,kill=kill,
                fts_environment={key:env[key] for key in ['LANCE_FTS_NUM_SHARDS','LANCE_FTS_PARTITION_SIZE']}))
            elapsed=time.monotonic()-started
            publish(output/'progress.json',dict(completed=len(records),total=len(steps),elapsed_s=elapsed,rate_per_s=len(records)/elapsed,eta_s=(len(steps)-len(records))*elapsed/len(records)))
    publish(output/'result.json',dict(status='passed',records=records,contenders=contenders,checks=len(records),seconds=time.monotonic()-started,temporary_root_removed=not pathlib.Path(temporary).exists(),artifact_sha256={p.name:digest(p) for p in output.iterdir() if p.is_file()},not_proven=['full corpus','concurrent reindex workload','power loss']))
    print(json.dumps(dict(status='passed',checks=len(records),seconds=time.monotonic()-started)))


if __name__=='__main__':main()

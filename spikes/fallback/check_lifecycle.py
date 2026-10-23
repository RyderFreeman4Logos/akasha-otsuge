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
    steps = []
    def add(channel, request, check=lambda _: True, *, error=None, kill=None):
        steps.append((channel, request, check, error, kill))
    def g(op, check=lambda _: True, generation=1, **fields):
        error, kill = fields.pop('error',None), fields.pop('kill',None)
        add('generation',dict(op=op,generation=generation,**fields),check,error=error,kill=kill)
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
    add('load',{})
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
    commit(2,2,1);commit(3,3,2)
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
    records, started = [], time.monotonic()
    with tempfile.TemporaryDirectory(prefix='akasha-fallback-lifecycle-') as temporary:
        root=pathlib.Path(temporary);database=root/'database';request=root/'request.json'
        (root/'OWNER').write_text('akasha-otsuge MVP-003 owned corpus import and database\n')
        raw=b'{"key":"baseline","body":"baseline content","partition":"p7"}\n'
        (root/'input.jsonl').write_bytes(raw)
        publish(root/'manifest.json',dict(corpus=dict(documents=1,vectors=0,prepared_bytes=len(raw),prepared_sha256=hashlib.sha256(raw).hexdigest(),indexed_body_utf8_bytes=16)))
        publish(output/'admission.json',dict(binary_sha256=digest(binary),source_sha256=digest(source),harness_sha256=digest(__file__),temporary_root=temporary,controller=process_identity(os.getpid()),planned_commands=len(steps)))
        for channel, value, check, error, kill in steps:
            name=f'{len(records):03d}';publish(request,value)
            args=['--load',str(root/'input.jsonl'),str(root/'manifest.json')] if channel=='load' else ['--'+channel,str(request)]
            command=[str(binary),str(database),*args]
            child=subprocess.Popen(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
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
                    assert process_identity(child.pid)['start_ticks']==identity['start_ticks'];child.kill()
                stdout,stderr=child.communicate(timeout=180)
                (output/(name+'.stdout')).write_bytes(stdout);(output/(name+'.stderr')).write_bytes(prefix+stderr)
                assert child.returncode==(-9 if kill else 1 if error else 0),(name,stderr[-2000:])
                if error:assert error.encode() in stderr,(name,stderr[-2000:])
                if not error and not kill:assert check(json.loads(stdout)),(name,stdout)
                if channel=='generation' and value['op']=='catch_up' and not error and not kill:
                    progress=json.loads((database/f"generation-{value['generation']}-catch-up.json").read_text())
                    assert progress['completed']>progress['completed_at_start']
                    assert abs(progress['rate_per_s']-(progress['completed']-progress['completed_at_start'])/progress['elapsed_s'])<1e-6
            finally:
                if child.poll() is None:child.kill()
                child.communicate(timeout=10)
            assert process_identity(child.pid) is None
            records.append(dict(command=command,request=value,child=identity,exit=child.returncode,error=error,kill=kill))
            elapsed=time.monotonic()-started
            publish(output/'progress.json',dict(completed=len(records),total=len(steps),elapsed_s=elapsed,rate_per_s=len(records)/elapsed,eta_s=(len(steps)-len(records))*elapsed/len(records)))
    publish(output/'result.json',dict(status='passed',records=records,checks=len(records),seconds=time.monotonic()-started,temporary_root_removed=not pathlib.Path(temporary).exists(),artifact_sha256={p.name:digest(p) for p in output.iterdir() if p.is_file()},not_proven=['full corpus','concurrent reindex workload','power loss']))
    print(json.dumps(dict(status='passed',checks=len(records),seconds=time.monotonic()-started)))


if __name__=='__main__':main()

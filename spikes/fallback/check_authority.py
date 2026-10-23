#!/usr/bin/env python3
"""Native authority transactions, immutable history and current authorization."""
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
    binary, output = [pathlib.Path(p).resolve() for p in sys.argv[1:]]
    output.mkdir(mode=0o700)
    (output / 'OWNER').write_text('akasha-otsuge MVP-003 fallback authority receipts\n')
    started, records = time.monotonic(), []
    with tempfile.TemporaryDirectory(prefix='akasha-fallback-authority-') as temporary:
        root = pathlib.Path(temporary)
        (root / 'OWNER').write_text('akasha-otsuge MVP-003 owned corpus import and database\n')
        database, source, manifest, request = [root / p for p in ('database','source.jsonl','manifest.json','request.json')]
        rows = [dict(key=f'fixture:source:{i}', body=f'Original source body {i}', partition=f'p{i}') for i in range(3)]
        raw = b''.join((json.dumps(row,separators=(',',':'))+'\n').encode() for row in rows)
        source.write_bytes(raw)
        manifest.write_text(json.dumps(dict(corpus=dict(documents=3,vectors=0,prepared_bytes=len(raw),
            prepared_sha256=hashlib.sha256(raw).hexdigest(),indexed_body_utf8_bytes=sum(len(x['body']) for x in rows)))))
        publish(output/'admission.json',dict(binary_sha256=digest(binary),harness_sha256=digest(pathlib.Path(__file__)),
            controller=process_identity(os.getpid()),temporary_root=temporary))

        def invoke(*args, kill=None, error=None):
            command = [str(binary),str(database),*map(str,args)]
            name = f'{len(records):03d}'
            child = subprocess.Popen(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE,
                env={**os.environ,'AKASHA_SPIKE_PROFILE':'low'})
            identity, prefix = process_identity(child.pid), b''
            try:
                if kill:
                    with selectors.DefaultSelector() as selector:
                        selector.register(child.stderr,selectors.EVENT_READ)
                        until=time.monotonic()+60
                        while time.monotonic()<until:
                            assert child.poll() is None,prefix
                            if selector.select(timeout=1):
                                prefix+=os.read(child.stderr.fileno(),4096)
                                if f'PROBE_CHECKPOINT:{kill}\n'.encode() in prefix:break
                        else:raise TimeoutError('authority commit barrier missing')
                    assert process_identity(child.pid)['start_ticks']==identity['start_ticks']
                    child.kill()
                stdout,stderr=child.communicate(timeout=60)
                (output/(name+'.stdout')).write_bytes(stdout)
                (output/(name+'.stderr')).write_bytes(prefix+stderr)
                assert child.returncode==(-9 if kill else 1 if error else 0),(name,stderr[-2000:])
                if error:assert error.encode() in stderr,(name,stderr)
                if kill!='after-reply' and kill:assert not stdout
                if kill=='after-reply':assert json.loads(stdout)['stage']=='committed'
            finally:
                if child.poll() is None:child.kill()
                child.communicate(timeout=10)
            assert process_identity(child.pid) is None
            records.append(dict(command=command,child=identity,exit=child.returncode,
                request=json.loads(request.read_text()) if args[0]=='--authority' else None,
                kill=kill,expected_error=error))
            elapsed=time.monotonic()-started
            publish(output/'progress.json',dict(completed=len(records),total=53,elapsed_s=elapsed,
                rate_per_s=len(records)/elapsed,eta_s=(53-len(records))*elapsed/len(records),**process_identity(os.getpid())))
            if not kill and not error:return json.loads(stdout)

        def call(op, **fields):
            kill,error=fields.pop('kill',None),fields.pop('error',None)
            publish(request,dict(op=op,**fields))
            return invoke('--authority',request,kill=kill,error=error)

        assert invoke('--load',source,manifest)['documents']==3
        assert call('status')['commit_seq']==0
        original=call('get',memory=1_000_000,at=0)
        assert original['seq']==0 and original['body']==rows[0]['body']
        assert call('current',principal='reader',memory=1_000_000)['entry'] is None
        assert call('permission',principal='reader',scope=0,allowed=True)['revision']==1
        assert call('permission',principal='reader',scope=0,allowed=True)['revision']==1
        assert call('current',principal='reader',memory=1_000_000)['entry']==original
        change=dict(request_id='first',memory=1_000_000,expected=0,scope=0,active=True,body='Corrected astronomy',text_only=True)
        call('apply',change=change,hold='before-commit',kill='before-commit')
        assert call('status')['commit_seq']==0
        assert call('receipt',request_id='first') is None
        assert call('job',seq=1) is None
        assert call('get',memory=1_000_000,at=0)==original
        call('apply',change=change,hold='after-commit',kill='after-commit')
        committed=call('receipt',request_id='first')
        assert committed['commit_seq']==1 and committed['stage']=='committed'
        assert call('apply',change=change)==committed
        assert call('apply',change=dict(change,body='conflicting'))==dict(conflict='idempotency')
        assert call('apply',change=dict(change,request_id='stale'))==dict(conflict='expected_revision',actual=1)
        assert call('job',seq=1)['revision']==committed['revision']
        first=call('get',memory=1_000_000,at=1)
        assert first['body']==change['body'] and first['previous']==0 and first['text_only']
        assert call('get',memory=1_000_000,at=0)==original
        changed=dict(change,request_id='second',expected=1,scope=1,body='Moved source')
        call('apply',change=changed,hold='after-reply',kill='after-reply')
        assert call('apply',change=changed)['commit_seq']==2
        assert call('current',principal='reader',memory=1_000_000)['entry'] is None
        assert call('state',scope=0,at=0,cursor=None,limit=100)['entries']==[original]
        assert call('state',scope=0,at=1,cursor=None,limit=100)['entries']==[first]
        assert call('state',scope=0,at=2,cursor=None,limit=100)['entries']==[]
        page=call('state',scope=1,at=2,cursor=None,limit=1)
        assert [e['memory'] for e in page['entries']]==[1_000_000] and page['next']==1_000_000
        page=call('state',scope=1,at=2,cursor=page['next'],limit=1)
        assert [e['memory'] for e in page['entries']]==[1_000_001]
        assert call('state',scope=1,at=2,cursor=page['next'],limit=1)['entries']==[]
        hit=dict(memory=1_000_000,seq=2,partition='p1',source_key='forged',body='untrusted derived body')
        assert call('revalidate',principal='reader',scope=1,read_seq=2,min_commit_seq=2,hits=[hit])['hits']==[]
        call('permission',principal='reader',scope=1,allowed=True)
        checked=call('revalidate',principal='reader',scope=1,read_seq=1,min_commit_seq=2,hits=[hit])
        assert checked['hits'][0]['source_key']=='fixture:source:0'
        assert 'body' not in checked['hits'][0]
        assert call('revalidate',principal='reader',scope=1,read_seq=2,min_commit_seq=2,hits=[dict(hit,seq=1)])['hits']==[]
        call('revalidate',principal='reader',scope=1,read_seq=2,min_commit_seq=3,hits=[hit],error='min_commit_seq_not_reached')
        call('apply',change=dict(changed,request_id='third',expected=2,active=False))
        assert call('current',principal='reader',memory=1_000_000)['entry'] is None
        assert call('state',scope=1,at=3,cursor=None,limit=100)['entries'][0]['memory']==1_000_001
        call('apply',change=dict(changed,request_id='fourth',expected=3))
        assert call('current',principal='reader',memory=1_000_000)['entry']['seq']==4
        assert call('current',principal='other',memory=1_000_000)['entry'] is None
        history=call('history',principal='reader',scope=1,at=2,cursor=None,limit=100)
        assert history['entries'][0]['seq']==2
        call('permission',principal='reader',scope=1,allowed=False)
        assert call('history',principal='reader',scope=1,at=2,cursor=None,limit=100)['entries']==[]
        assert call('get',memory=1_000_000,at=2)['seq']==2
        for invalid in [dict(change,request_id=''),dict(change,scope=8),dict(change,body='x'*131073)]:
            call('apply',change=invalid,error='invalid change')
        call('apply',change=dict(change,request_id='escaped',expected=4,body='\x01'*100000),error='authority record exceeds 512 KiB')
        call('get',memory=1_000_000,at=5,error='historical read point is not committed')
        call('permission',principal='../reader',scope=0,allowed=True,error='invalid permission principal or scope')
        call('state',scope=8,at=4,cursor=None,limit=100,error='invalid history page')
        assert call('status')['commit_seq']==4
        assert invoke('verify-import')['documents']==3
        assert len(records)==53,len(records)
    publish(output/'result.json',dict(status='passed',checks=len(records),seconds=time.monotonic()-started,
        records=records,temporary_root_removed=not pathlib.Path(temporary).exists(),
        artifact_sha256={p.name:digest(p) for p in output.iterdir() if p.is_file()},
        not_proven=['100k changes','concurrent readers/writers','full corpus','derived outbox application','power loss']))
    print(json.dumps(dict(status='passed',checks=len(records))))


if __name__=='__main__':main()

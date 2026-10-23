#!/usr/bin/env python3
"""Real generation indexes, gap-free progress and process-crash recovery."""
import hashlib
import json
import pathlib
import os
import re
import selectors
import subprocess
import sys
import tempfile
import time
from run_case import digest, process_identity, publish


def main():
    binary, source = [pathlib.Path(v).resolve(strict=True) for v in sys.argv[1:3]]
    output = pathlib.Path(sys.argv[3]).resolve()
    output.mkdir(mode=0o700)
    (output/'OWNER').write_text('akasha-otsuge MVP-003 owned generation diagnostic receipts\n')
    if source.stat().st_size>4*1024**2:raise ValueError('generation source subset exceeds 4 MiB')
    rows = [json.loads(line) for line in source.read_text().splitlines()]
    assert len(rows)==8
    records=[]
    planned_commands=96
    publish(output/'admission.json',{'controller':process_identity(os.getpid()),'binary_sha256':digest(binary),'harness_sha256':digest(pathlib.Path(__file__)),'source_sha256':digest(source),'planned_commands':planned_commands})
    started=time.monotonic()
    with tempfile.TemporaryDirectory(prefix='akasha-generations-') as directory:
        database=pathlib.Path(directory)/'database'
        database.mkdir(mode=0o700)
        (pathlib.Path(directory)/'OWNER').write_text('akasha-otsuge MVP-003 disposable generation database\n')
        admission=json.loads((output/'admission.json').read_text());admission['temporary_root']=directory;publish(output/'admission.json',admission)
        def invoke(request, *, kill=None, failure=False):
            name=f'{len(records):03d}-{request["op"]}'
            if kill:request={**request,'hold':kill}
            path=output/(name+'.json');path.write_text(json.dumps(request))
            with (output/(name+'.log')).open('x') as log:
                command=[str(binary),str(database),request['cypher']] if request['op']=='raw' else [str(binary),str(database),'--generation',str(path)]
                child=subprocess.Popen(command,stdout=log,stderr=subprocess.PIPE,bufsize=0)
                child_identity=process_identity(child.pid)
                try:
                    if kill:
                        stderr=b''
                        with selectors.DefaultSelector() as selector:
                            selector.register(child.stderr,selectors.EVENT_READ)
                            deadline=time.monotonic()+90
                            while time.monotonic()<deadline:
                                assert child.poll() is None, stderr+child.stderr.read()
                                if selector.select(timeout=1):
                                    stderr+=os.read(child.stderr.fileno(),4096)
                                    if ('PROBE_CHECKPOINT:'+kill+'\n').encode() in stderr:break
                            else:raise TimeoutError(kill)
                        child.kill();_,tail=child.communicate(timeout=10);stderr+=tail
                        assert child.returncode==-9
                        text=stderr.decode(errors='replace')
                    else:
                        _,raw=child.communicate(timeout=120)
                        text=raw.decode(errors='replace')
                finally:
                    if child.poll() is None:child.kill()
                    child.communicate(timeout=10)
            (output/(name+'.stderr')).write_text(text)
            record={'request':path.name,'command':command,'child':child_identity,'exit':child.returncode,'hold':kill,'expected_failure':failure}
            records.append(record)
            publish(output/'progress.json',{'phase':name,'completed':len(records),'total':planned_commands,'elapsed_s':time.monotonic()-started,'rate_per_s':len(records)/(time.monotonic()-started),'eta_s':(planned_commands-len(records))*(time.monotonic()-started)/len(records)})
            if not kill:assert (child.returncode!=0)==failure,text
            if failure or kill:return None
            result=json.loads((output/(name+'.log')).read_text().splitlines()[-1]);record['result']=result;return result
        model_identity={'model':'text-embedding-3-large','repository':'Qdrant/dbpedia-entities-openai3-text-embedding-3-large-1536-1M','revision':'4a9731217921bc476a0f03544f11f22ae4903fa5','precomputed_at':'2024-02','precision':'float32'}
        fingerprint=hashlib.sha256(json.dumps(model_identity,sort_keys=True).encode()).hexdigest()
        bundle={'model':model_identity['model'],'model_fingerprint':fingerprint,'dimension':1536,'chunker':'source-document-v1','schema':1}
        def begin(g=1, **updates):return invoke({'op':'begin','generation':g,'bundle':{**bundle,**updates}})
        def commit(seq,memory,row,expected=0,active=True):
            value={'request_id':f'g-{seq}','memory':memory,'expected':expected,'scope':int(rows[row]['partition'][1]),'active':active,'body':rows[row]['body']}
            assert invoke({'op':'commit','change':value})['commit_seq']==seq
        def apply(g,f,seq,row=None,**kwargs):
            output=None if row is None else {'model':bundle['model'],'model_fingerprint':fingerprint,'body_sha256':hashlib.sha256(rows[row]['body'].encode()).hexdigest(),'values':rows[row]['embedding']}
            return invoke({'op':'apply','generation':g,'family':f,'seq':seq,'vector':output},**kwargs)
        def receipt(seq,g=1):return invoke({'op':'receipt','generation':g,'request_id':f'g-{seq}'})
        def query(g,row=0,*,text=False,**updates):
            clause={'term':max(re.findall(r'[A-Za-z]{4,}',rows[row]['body']),key=len)} if text else {'vector':rows[row]['embedding']}
            return invoke({'op':'query','generation':g,'bundle':{**bundle,**updates},'principal':'reader','scope':int(rows[row]['partition'][1]),**clause})
        begin()
        for scope in range(8):invoke({'op':'grant','principal':'reader','scope':scope,'allowed':True})
        invoke({'op':'build','generation':1});commit(1,1,0)
        assert receipt(1)['stage']=='committed'
        good={'model':bundle['model'],'model_fingerprint':fingerprint,'body_sha256':hashlib.sha256(rows[0]['body'].encode()).hexdigest(),'values':rows[0]['embedding']}
        for change in [{'model':'wrong-model'},{'model_fingerprint':'0'*64},{'body_sha256':'0'*64},{'values':[1.0,2.0,3.0]}]:
            invoke({'op':'apply','generation':1,'family':'vector','seq':1,'vector':{**good,**change}},failure=True)
        invoke({'op':'apply','generation':1,'family':'text','seq':1,'vector':good},failure=True)
        apply(1,'vector',1,failure=True)
        assert receipt(1)['stage']=='committed'
        for f in ['text','vector']:apply(1,f,1,0 if f=='vector' else None)
        assert receipt(1)['stage']=='indexed'
        invoke({'op':'activate','generation':1})
        assert query(1)['hits'][0]['memory']==1
        assert query(1,text=True)['hits'][0]['memory']==1
        commit(2,2,1);commit(3,3,2)
        for f in ['text','vector']:apply(1,f,3,2 if f=='vector' else None)
        assert receipt(3)['watermarks']=={'text':1,'vector':1}
        for f in ['text','vector']:apply(1,f,2,1 if f=='vector' else None)
        assert receipt(3)['watermarks']=={'text':3,'vector':3}
        commit(4,1,3,1);commit(5,1,3,4,False)
        for seq in [4,5]:
            for f in ['text','vector']:apply(1,f,seq)
        assert receipt(4)['reasons']=={'text':'superseded','vector':'superseded'}
        assert query(1)['hits']==[]
        commit(6,1,0,5)
        apply(1,'text',6,kill='after-index-application')
        assert query(1,text=True)['hits'][0]['seq']==6
        assert receipt(6)['stage']=='committed'
        apply(1,'text',6)
        apply(1,'vector',6,0,kill='before-watermark')
        assert receipt(6)['watermarks']=={'text':6,'vector':5}
        apply(1,'vector',6,0)
        assert receipt(6)['watermarks']=={'text':6,'vector':6}
        assert query(1)['hits'][0]['seq']==6
        begin(2,schema=2)
        invoke({'op':'build','generation':2,'fail_after_text':True},failure=True)
        assert invoke({'op':'status'})['active']==1
        assert query(1,text=True)['hits'][0]['seq']==6
        assert query(1)['hits'][0]['seq']==6
        change={'request_id':'g-7','memory':4,'expected':0,'scope':int(rows[4]['partition'][1]),'active':True,'body':rows[4]['body']}
        built=invoke({'op':'build','generation':2,'during_build':change})
        assert built['concurrent_commit']['commit_seq']==7
        invoke({'op':'activate','generation':2},failure=True)
        for seq,row in [(1,None),(2,1),(3,2),(4,None),(5,None),(6,0),(7,4)]:
            for f in ['text','vector']:apply(2,f,seq,row if f=='vector' else None)
        invoke({'op':'activate','generation':2},kill='before-activation')
        assert invoke({'op':'status'})['active']==1
        invoke({'op':'activate','generation':2},kill='after-activation')
        assert invoke({'op':'status'})['active']==2
        assert query(2,schema=2)['hits'][0]['seq']==6
        assert query(2,text=True,schema=2)['hits'][0]['seq']==6
        assert query(1)['hits'][0]['seq']==6
        invoke({'op':'query','generation':2,'bundle':bundle,'principal':'reader','scope':0,'vector':rows[0]['embedding']},failure=True)
        invoke({'op':'begin','generation':3,'bundle':bundle},failure=True)
        changed={'model':'wrong-model','model_fingerprint':fingerprint,'body_sha256':hashlib.sha256(rows[0]['body'].encode()).hexdigest(),'values':rows[0]['embedding']}
        invoke({'op':'apply','generation':2,'family':'vector','seq':6,'vector':changed},failure=True)
        invoke({'op':'query','generation':2,'bundle':{**bundle,'schema':2,'model_fingerprint':'0'*64},'principal':'reader','scope':0,'vector':rows[0]['embedding']},failure=True)
        invoke({'op':'raw','cypher':"MATCH (p:IndexState {key:'active'}) SET p.generation=0"})
        invoke({'op':'status'},failure=True)
        invoke({'op':'raw','cypher':"MATCH (p:IndexState {key:'active'}) SET p.generation=2"})
        invoke({'op':'raw','cypher':"MATCH (g:Generation {key:1}) SET g.status='invalid'"})
        invoke({'op':'status'},failure=True)
        invoke({'op':'raw','cypher':"MATCH (g:Generation {key:1}) SET g.status='retired'"})
        invoke({'op':'raw','cypher':"CREATE (:IndexedJob {key:'00000000000000000001:text:00000000000000000007',generation:1,family:'text',seq:8,reason:'applied',hash:'corrupt fixture'})"})
        invoke({'op':'advance','generation':1,'family':'text'},failure=True)
        invoke({'op':'raw','cypher':"MATCH (j:IndexedJob {key:'00000000000000000001:text:00000000000000000007'}) DELETE j"})
        assert invoke({'op':'advance','generation':2,'family':'text'})['advanced']==0
        final=invoke({'op':'status'})
        invoke({'op':'raw','cypher':"CREATE (:Document {key:'baseline-requires-backfill'})"})
        invoke({'op':'begin','generation':1,'bundle':bundle},failure=True)
    assert len(records)==planned_commands
    publish(output/'progress.json',{'phase':'completed','completed':len(records),'total':planned_commands,'elapsed_s':time.monotonic()-started,'rate_per_s':len(records)/(time.monotonic()-started),'eta_s':0})
    result={'status':'passed','binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'seconds':time.monotonic()-started,'records':records,'final':final,'not_tested':['full corpus backfill','30-minute profiles','physical derived corruption','power loss','disk budget for full reindex']}
    assert result['binary_sha256']==admission['binary_sha256']
    assert digest(pathlib.Path(__file__))==admission['harness_sha256']
    result['artifact_sha256']={p.name:digest(p) for p in sorted(output.iterdir()) if p.is_file()}
    publish(output/'result.json',result)
    print(json.dumps({'status':'passed','steps':len(records),'seconds':result['seconds']}))


if __name__=='__main__':main()

import hashlib, json, sqlite3, struct, time
from pathlib import Path
import importlib.util

E = Path(__file__).parent
spec = importlib.util.spec_from_file_location('source_oracle', E / 'mvp025-mempal-real-audit-r1.py')
source_oracle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(source_oracle)


def decode(cell):
    tag = cell['type']
    if tag == 'null':
        assert set(cell) == {'type'}
        return None
    assert set(cell) == {'type', 'value'}
    value = cell['value']
    if tag == 'text': assert isinstance(value, str); return value
    if tag == 'integer': assert isinstance(value, int) and not isinstance(value, bool); return value
    if tag == 'real': assert len(value) == 16; return struct.unpack('>d', bytes.fromhex(value))[0]
    if tag == 'blob': return bytes.fromhex(value)
    raise AssertionError('unknown typed value')


def self_check():
    assert decode({'type':'null'}) is None
    assert decode({'type':'text','value':'原\0文'}) == '原\0文'
    assert decode({'type':'blob','value':'00ff'}) == b'\0\xff'
    assert decode({'type':'integer','value':-3}) == -3
    assert decode({'type':'real','value':'3fe0000000000000'}) == 0.5
    source_oracle.self_check()
    print('archive typed-cell oracle self-check PASS')


def audit(archive, snapshot, source_hash, progress, plan, expected):
    manifest = json.loads((archive / 'manifest.json').read_text())
    assert set(manifest['files']) == {'blobs.bin','commits.jsonl'}
    db = sqlite3.connect(snapshot.as_uri()+'?mode=ro&immutable=1', uri=True)
    db.execute('pragma query_only=on'); db.execute('pragma trusted_schema=off')
    samples = {}
    for table,field,suffix in [('drawers','content',''),('knowledge_cards','statement',''),('knowledge_cards','content','/content'),('conversation_turns','content','')]:
        for order in ['ASC','DESC']:
            for rowid,text in db.execute(f'SELECT rowid,"{field}" FROM "{table}" ORDER BY rowid {order} LIMIT 3'):
                samples[f'mempal/{source_hash}/{table}/{rowid}{suffix}'] = {'text':text,'chunks':{}}
    db.close()
    tables = {name:{'digest':hashlib.sha256(b'mempal-table-v1\0'+json.dumps(columns,separators=(',',':')).encode()),'raw_digest':hashlib.sha256(),'pending':'','rows':0,'utf8_bytes':0,'blob_bytes':0,'bytes':0,'blocks':0,'last_rowid':None,'descriptor':None} for name,columns in plan.items()}
    prefix = f'mempal/{source_hash}/table/'
    seq = revisions = offset = native_sources = native_memories = native_reviews = 0
    digest_commits = hashlib.sha256(); digest_blobs = hashlib.sha256(); migration = None; last_progress=time.monotonic()
    with (archive/'commits.jsonl').open('rb') as commits, (archive/'blobs.bin').open('rb') as blobs:
        while line := commits.readline(1024*1024+1):
            assert len(line)<=1024*1024 and line.endswith(b'\n')
            digest_commits.update(line); entry=json.loads(line); seq+=1
            assert entry['receipt']['commit_seq']==seq
            originals={}; memories={}; reviews=[]
            assert len(entry['revisions'])<=64
            for revision in entry['revisions']:
                length=revision['blob_bytes']; assert 0 < length <= 1024*1024
                assert revision['blob_offset']==offset and revision['commit_seq']==seq and revision['previous'] is None
                raw=blobs.read(length); assert len(raw)==length and hashlib.sha256(raw).hexdigest()==revision['content_sha256']
                digest_blobs.update(raw); offset+=length; revisions+=1; value=json.loads(raw)
                kind=revision['key']['kind']; key=revision['key']['id']
                if kind=='evidence' and key.startswith(prefix):
                    tail=key[len(prefix):].split('/'); name=tail[0]; table=tables[name]
                    if len(tail)==1:
                        assert table['descriptor'] is None; table['descriptor']=value
                    else:
                        assert len(tail)==2 and int(tail[1])==table['blocks']
                        assert value['table']==name and value['start_byte']==table['bytes']
                        data=value['data']; encoded=data.encode(); table['bytes']+=len(encoded);table['blocks']+=1;table['raw_digest'].update(encoded)
                        parts=(table['pending']+data).split('\n');table['pending']=parts.pop()
                        for row in parts:
                            row=json.loads(row); assert set(row)=={'rowid','values'} and len(row['values'])==len(plan[name])
                            assert table['last_rowid'] is None or table['last_rowid']<row['rowid'];table['last_rowid']=row['rowid']
                            values=[row['rowid'],*[decode(cell) for cell in row['values']]]
                            table['digest'].update(source_oracle.row_digest(values));table['rows']+=1
                            table['utf8_bytes']+=sum(len(v.encode()) for v in values if isinstance(v,str))
                            table['blob_bytes']+=sum(len(v) for v in values if isinstance(v,bytes))
                elif kind=='evidence' and key==f'mempal/{source_hash}/migration':
                    assert migration is None;migration=value
                elif kind=='evidence' and key.startswith('source/'):
                    native_sources+=1; payload=value['payload']; event=payload['event_id']
                    assert value['provenance']=={'actor':{'kind':'unverified','id':'mempal'},'origin':'unverified','binding':f'snapshot:{source_hash}','generator':None}
                    assert key=='source/'+hashlib.sha256(event.encode()).hexdigest()
                    assert value['content_sha256']==hashlib.sha256(payload['text'].encode()).hexdigest()
                    assert payload['observed_time_ms']==entry['recorded_at_ms']
                    assert event not in originals;originals[event]=value
                    base,part,end=event.rsplit('/',2);assert end=='source'
                    if base in samples:
                        assert int(part) not in samples[base]['chunks'];samples[base]['chunks'][int(part)]=payload['text']
                elif kind=='memory':
                    native_memories+=1
                    assert value['authority']=='candidate' and value['lifecycle']=='candidate' and value['approval'] is None
                    assert value['provenance']['actor']['kind']=='unverified' and value['provenance']['origin']=='unverified'
                    assert key.startswith(f'mempal/{source_hash}/'); assert key not in memories; memories[key]=value
                elif kind=='state' and key.startswith('review/'):
                    native_reviews+=1;reviews.append(value)
                    assert value['state']=='open' and value['proposal_revision']==seq and value['proposal_request']==''
                else: raise AssertionError('unexpected activated record category')
            for value in memories.values():
                draft=value['draft']; assert len(draft['source_spans'])==1
                span=draft['source_spans'][0]; original=originals[span['event_id']]
                assert span['revision']==seq and span['content_sha256']==original['content_sha256']
                assert span['start']==0 and span['end']==len(original['payload']['text'].encode())
                assert draft['claim']==original['payload']['text'] and draft['scope']==original['payload']['scope']
            assert len(memories)==len(reviews)
            for review in reviews:
                assert review['semantic_sha256']==memories[review['memory_id']]['semantic_sha256']
                assert review['scope']==memories[review['memory_id']]['draft']['scope']
            if time.monotonic()-last_progress>=5:
                progress(seq,manifest['commits'],sum(t['rows'] for t in tables.values()));last_progress=time.monotonic()
        assert not blobs.read(1)
    assert seq==manifest['commit_seq']==manifest['commits'] and revisions==manifest['revisions']
    for name,digest in [('commits.jsonl',digest_commits),('blobs.bin',digest_blobs)]:
        assert digest.hexdigest()==manifest['files'][name]['sha256']
        assert (archive/name).stat().st_size==manifest['files'][name]['bytes']
    result={}
    for name,table in tables.items():
        assert not table['pending'] and table['descriptor']['columns']==plan[name]
        for key in ['rows','utf8_bytes','blob_bytes']: assert table[key]==expected[name][key],(name,key)
        for key in ['rows','blocks','bytes']: assert table[key]==table['descriptor'][key],(name,key)
        assert table['raw_digest'].hexdigest()==table['descriptor']['sha256']
        assert table['digest'].hexdigest()==expected[name]['typed_rows_sha256'],name
        result[name]={key:table[key] for key in ['rows','utf8_bytes','blob_bytes']}
    for sample in samples.values():
        parts=sample['chunks'];assert sorted(parts)==list(range(len(parts)))
        assert ''.join(parts[i] for i in range(len(parts)))==sample['text']
    assert migration['source_sha256']==source_hash
    assert migration['projection']['source_chunks']==native_sources and migration['projection']['candidate_memories']==native_memories==native_reviews
    return {'status':'PASS','tables':result,'commits':seq,'revisions':revisions,'native_sources':native_sources,'native_candidates':native_memories,'native_reviews':native_reviews,'sampled_original_fields':len(samples),'projection':migration['projection'],'source_relationship_evidence':'all retained typed rows match the independent SQLite row digests; includes seven previously audited relation categories','archive_bytes':sum(v['bytes'] for v in manifest['files'].values())}


if __name__=='__main__': self_check()
